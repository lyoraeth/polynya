// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Weather from Open-Meteo: current, hourly and daily, plus sunrise and sunset.
//!
//! Runs in its own thread. The place is the city from the config (resolved to coordinates once) or,
//! if no city is set, the Windows location: coordinates from the system, the name from the
//! OpenStreetMap reverse geocoder (Open-Meteo has none). The forecast refreshes on a timer.
//! Networking is WinHTTP, no third-party HTTP clients.

use std::{sync::Mutex, time::Duration};

use serde::Deserialize;
use windows::{
    Win32::Networking::WinHttp::{
        INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
        WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    },
    core::{Error, HSTRING, PCWSTR, Result, w},
};

/// How often to refresh the forecast and how soon to retry after an error.
const REFRESH: Duration = Duration::from_secs(20 * 60);
const RETRY: Duration = Duration::from_secs(60);
/// How many upcoming hours and days the expanded view shows.
pub const HOURS: usize = 6;
pub const DAYS: usize = 3;

/// Icon type by WMO weather code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sky {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Rain,
    Snow,
    Thunder,
}

#[derive(Debug, Clone)]
pub struct Now {
    pub temp: f32,
    pub feels: f32,
    pub humidity: f32,
    pub wind: f32,
    pub code: u16,
    pub is_day: bool,
}

#[derive(Debug, Clone)]
pub struct Hour {
    pub hour: u8,
    pub temp: f32,
    pub code: u16,
    pub is_day: bool,
}

#[derive(Debug, Clone)]
pub struct Day {
    /// Weekday: 0 is Sunday.
    pub dow: u8,
    pub min: f32,
    pub max: f32,
    pub code: u16,
}

#[derive(Debug, Clone)]
pub struct Weather {
    pub city: String,
    /// The place comes from the Windows location, not the config.
    pub located: bool,
    pub now: Now,
    pub hours: Vec<Hour>,
    pub days: Vec<Day>,
    /// Today's sunrise and sunset, "HH:MM".
    pub sunrise: String,
    pub sunset: String,
}

static STATE: Mutex<Option<Weather>> = Mutex::new(None);
/// New city for the weather thread, from the settings, without a restart.
static CITY: Mutex<Option<std::sync::mpsc::Sender<Option<String>>>> = Mutex::new(None);

/// Changes the city on the fly: the weather thread rereads the place and forecast at once.
pub fn set_city(city: Option<String>) {
    if let Some(tx) = CITY.lock().ok().and_then(|c| c.clone()) {
        let _ = tx.send(city);
    }
}

pub fn current() -> Option<Weather> {
    STATE.lock().ok().and_then(|s| s.clone())
}

/// Starts the weather thread. `notify` is called from it on every update.
pub fn start(city: Option<String>, notify: fn()) {
    let (tx, rx) = std::sync::mpsc::channel();
    if let Ok(mut c) = CITY.lock() {
        *c = Some(tx);
    }
    std::thread::Builder::new()
        .name("weather".into())
        .spawn(move || {
            // Geolocation is WinRT.
            // SAFETY: initializes COM for this new thread before any WinRT call on it.
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
            }
            let mut place = None;
            let mut city = city;
            loop {
                let wait = match refresh(city.as_deref(), &mut place) {
                    Ok(w) => {
                        if let Ok(mut s) = STATE.lock() {
                            *s = Some(w);
                        }
                        notify();
                        REFRESH
                    }
                    // Location is denied or off: wait until the next scheduled time.
                    Err(e) if e.code() == NO_LOCATION => {
                        proto::warn!("weather", "location unavailable and no city set");
                        REFRESH
                    }
                    Err(e) => {
                        proto::warn!("weather", "{e}");
                        RETRY
                    }
                };
                // Until the next refresh, or until the city changes.
                if let Ok(new) = rx.recv_timeout(wait) {
                    city = new;
                    place = None;
                }
            }
        })
        .ok();
}

/// Coordinates and the confirmed city name.
#[derive(Clone)]
struct Place {
    lat: f64,
    lon: f64,
    name: String,
}

fn refresh(city: Option<&str>, place: &mut Option<Place>) -> Result<Weather> {
    match city {
        Some(city) if place.is_none() => *place = Some(geocode(city)?),
        Some(_) => {}
        // A laptop may have moved: coordinates every time, the name only after a move.
        None => {
            let (lat, lon) = locate()?;
            let moved = place.as_ref().is_none_or(|p| (p.lat - lat).abs() > MOVED || (p.lon - lon).abs() > MOVED);
            if moved {
                let name = reverse(lat, lon).unwrap_or_default();
                *place = Some(Place { lat, lon, name });
            }
        }
    }
    let p = place.clone().unwrap();
    let path = format!(
        "/v1/forecast?latitude={:.4}&longitude={:.4}&timezone=auto&forecast_days={}&wind_speed_unit=ms\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day\
         &hourly=temperature_2m,weather_code,is_day&daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset",
        p.lat,
        p.lon,
        DAYS + 1
    );
    let body = get("api.open-meteo.com", &path)?;
    let r: Forecast = serde_json::from_str(&body).map_err(|e| Error::new(windows::core::HRESULT(-1), e.to_string()))?;
    let mut w = build(p.name, r);
    w.located = city.is_none();
    Ok(w)
}

/// The "no location" error: access denied or the service is off.
const NO_LOCATION: windows::core::HRESULT = windows::core::HRESULT(0x8007_0005u32 as i32);
/// Coordinate shift after which the place name is asked again, degrees (~5 km).
const MOVED: f64 = 0.05;

/// Coordinates from the system geolocation. City precision: coordinates are rounded to hundredths
/// (~1 km), and only those go to the network.
fn locate() -> Result<(f64, f64)> {
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};
    if Geolocator::RequestAccessAsync()?.join()? != GeolocationAccessStatus::Allowed {
        return Err(Error::from_hresult(NO_LOCATION));
    }
    let pos = Geolocator::new()?.GetGeopositionAsync()?.join().map_err(|_| Error::from_hresult(NO_LOCATION))?;
    let p = pos.Coordinate()?.Point()?.Position()?;
    let round = |v: f64| (v * 100.0).round() / 100.0;
    Ok((round(p.Latitude), round(p.Longitude)))
}

/// Place name by coordinates: a city, town or district.
fn reverse(lat: f64, lon: f64) -> Result<String> {
    let path = format!("/reverse?lat={lat:.2}&lon={lon:.2}&zoom=10&format=json&accept-language={}", crate::i18n::t().api_lang);
    let body = get("nominatim.openstreetmap.org", &path)?;
    #[derive(Deserialize)]
    struct Address {
        city: Option<String>,
        town: Option<String>,
        village: Option<String>,
        municipality: Option<String>,
    }
    #[derive(Deserialize)]
    struct Hit {
        address: Option<Address>,
        name: Option<String>,
    }
    let hit: Hit = serde_json::from_str(&body).map_err(|e| Error::new(windows::core::HRESULT(-1), e.to_string()))?;
    let a = hit.address;
    Ok(a.and_then(|a| a.city.or(a.town).or(a.village).or(a.municipality)).or(hit.name).unwrap_or_default())
}

fn geocode(city: &str) -> Result<Place> {
    let path = format!("/v1/search?name={}&count=1&language={}&format=json", urlencode(city), crate::i18n::t().api_lang);
    let body = get("geocoding-api.open-meteo.com", &path)?;
    let r: Geo = serde_json::from_str(&body).map_err(|e| Error::new(windows::core::HRESULT(-1), e.to_string()))?;
    let hit = r.results.and_then(|v| v.into_iter().next()).ok_or_else(|| Error::new(windows::core::HRESULT(-1), format!("city \"{city}\" not found")))?;
    Ok(Place { lat: hit.latitude, lon: hit.longitude, name: hit.name })
}

#[derive(Deserialize)]
struct Geo {
    results: Option<Vec<GeoHit>>,
}

#[derive(Deserialize)]
struct GeoHit {
    name: String,
    latitude: f64,
    longitude: f64,
}

#[derive(Deserialize)]
struct Forecast {
    current: Current,
    hourly: Hourly,
    daily: Daily,
}

#[derive(Deserialize)]
struct Current {
    time: String,
    temperature_2m: f32,
    apparent_temperature: f32,
    relative_humidity_2m: f32,
    weather_code: u16,
    wind_speed_10m: f32,
    is_day: u8,
}

#[derive(Deserialize)]
struct Hourly {
    time: Vec<String>,
    temperature_2m: Vec<f32>,
    weather_code: Vec<u16>,
    is_day: Vec<u8>,
}

#[derive(Deserialize)]
struct Daily {
    time: Vec<String>,
    weather_code: Vec<u16>,
    temperature_2m_max: Vec<f32>,
    temperature_2m_min: Vec<f32>,
    sunrise: Vec<String>,
    sunset: Vec<String>,
}

/// Builds the model from the reply. Times in the reply are local to the city (`timezone=auto`).
fn build(city: String, r: Forecast) -> Weather {
    // Hours after the current one; time strings are "YYYY-MM-DDTHH:MM", compared by hour.
    let now_hour = &r.current.time[..r.current.time.len().min(13)];
    let start = r.hourly.time.iter().position(|t| t.starts_with(now_hour)).map_or(0, |i| i + 1);
    let hours = (start..r.hourly.time.len())
        .take(HOURS)
        .map(|i| Hour {
            hour: r.hourly.time[i].get(11..13).and_then(|h| h.parse().ok()).unwrap_or(0),
            temp: r.hourly.temperature_2m[i],
            code: r.hourly.weather_code[i],
            is_day: r.hourly.is_day.get(i).copied().unwrap_or(1) == 1,
        })
        .collect();
    // Days from tomorrow: today is already described by the current weather.
    let days = (1..r.daily.time.len())
        .take(DAYS)
        .map(|i| Day { dow: weekday(&r.daily.time[i]), min: r.daily.temperature_2m_min[i], max: r.daily.temperature_2m_max[i], code: r.daily.weather_code[i] })
        .collect();
    let hm = |s: Option<&String>| s.and_then(|s| s.get(11..16)).unwrap_or("—").to_string();
    Weather {
        city,
        located: false,
        now: Now {
            temp: r.current.temperature_2m,
            feels: r.current.apparent_temperature,
            humidity: r.current.relative_humidity_2m,
            wind: r.current.wind_speed_10m,
            code: r.current.weather_code,
            is_day: r.current.is_day == 1,
        },
        hours,
        days,
        sunrise: hm(r.daily.sunrise.first()),
        sunset: hm(r.daily.sunset.first()),
    }
}

/// Weekday by a "YYYY-MM-DD" date (Sakamoto's algorithm), 0 is Sunday.
fn weekday(date: &str) -> u8 {
    let p = |a: usize, b: usize| date.get(a..b).and_then(|s| s.parse::<i32>().ok()).unwrap_or(1);
    let (mut y, m, d) = (p(0, 4), p(5, 7), p(8, 10));
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    if m < 3 {
        y -= 1;
    }
    ((y + y / 4 - y / 100 + y / 400 + T[(m - 1).clamp(0, 11) as usize] + d).rem_euclid(7)) as u8
}

/// WMO weather code → icon.
pub fn sky(code: u16) -> Sky {
    match code {
        0 | 1 => Sky::Clear,
        2 => Sky::PartlyCloudy,
        3 => Sky::Cloudy,
        45 | 48 => Sky::Fog,
        71..=77 | 85 | 86 => Sky::Snow,
        95..=99 => Sky::Thunder,
        51..=67 | 80..=82 => Sky::Rain,
        _ => Sky::Cloudy,
    }
}

/// Temperature with an explicit sign, as forecasts write it: "+6°", "−3°", "0°".
pub fn temp(t: f32) -> String {
    let r = t.round() as i32;
    match r {
        0 => "0°".into(),
        r if r > 0 => format!("+{r}°"),
        // A real minus, not a hyphen: the hyphen has a different width.
        r => format!("\u{2212}{}°", -r),
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// HTTPS GET, returning the response body as a string.
pub fn get(host: &str, path: &str) -> Result<String> {
    // SAFETY: every handle is checked for null and owned by a `Handle` guard that closes it once;
    // string arguments outlive their calls, and each read writes at most `avail` bytes into the
    // space just reserved in `body`.
    unsafe {
        let session = WinHttpOpen(w!("Polynya"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
        if session.is_null() {
            return Err(Error::from_thread());
        }
        let _s = Handle(session);
        // Without timeouts a hung request would hold the weather thread forever.
        WinHttpSetTimeouts(session, 10_000, 10_000, 10_000, 15_000)?;
        let conn = WinHttpConnect(session, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0);
        if conn.is_null() {
            return Err(Error::from_thread());
        }
        let _c = Handle(conn);
        let req = WinHttpOpenRequest(conn, w!("GET"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE);
        if req.is_null() {
            return Err(Error::from_thread());
        }
        let _r = Handle(req);
        WinHttpSendRequest(req, None, None, 0, 0, 0)?;
        WinHttpReceiveResponse(req, std::ptr::null_mut())?;
        let mut body = Vec::new();
        loop {
            let mut avail = 0u32;
            WinHttpQueryDataAvailable(req, &mut avail)?;
            if avail == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + avail as usize, 0);
            let mut read = 0u32;
            WinHttpReadData(req, body[start..].as_mut_ptr() as _, avail, &mut read)?;
            body.truncate(start + read as usize);
        }
        String::from_utf8(body).map_err(|e| Error::new(windows::core::HRESULT(-1), e.to_string()))
    }
}

/// Closes a WinHTTP handle when it goes out of scope.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a non-null WinHTTP handle owned by this guard and closed only here.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live system: geolocation and place name. `cargo test -- --ignored locate_live`
    #[test]
    #[ignore]
    fn locate_live() {
        // SAFETY: initializes COM for the test thread.
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let (lat, lon) = locate().expect("location");
        let name = reverse(lat, lon).expect("place name");
        println!("{lat:.2}, {lon:.2} — {name}");
        assert!(!name.is_empty());
    }

    #[test]
    fn weekday_known_dates() {
        assert_eq!(weekday("2026-10-18"), 0); // Sunday.
        assert_eq!(weekday("2026-10-19"), 1);
        assert_eq!(weekday("2024-02-29"), 4);
    }

    #[test]
    fn temperature_sign() {
        assert_eq!(temp(5.6), "+6°");
        assert_eq!(temp(-0.4), "0°");
        assert_eq!(temp(-3.2), "\u{2212}3°");
    }

    #[test]
    fn urlencode_cyrillic() {
        assert_eq!(urlencode("Москва"), "%D0%9C%D0%BE%D1%81%D0%BA%D0%B2%D0%B0");
    }

    #[test]
    fn build_takes_hours_after_now_and_days_after_today() {
        let json = r#"{
            "current": {"time":"2026-10-18T11:15","temperature_2m":6.2,"apparent_temperature":3.1,
                        "relative_humidity_2m":87,"weather_code":3,"wind_speed_10m":4.2,"is_day":1},
            "hourly": {"time":["2026-10-18T10:00","2026-10-18T11:00","2026-10-18T12:00","2026-10-18T13:00"],
                       "temperature_2m":[5,6,7,8],"weather_code":[3,3,61,2],"is_day":[1,1,1,1]},
            "daily": {"time":["2026-10-18","2026-10-19"],"weather_code":[3,61],
                      "temperature_2m_max":[8,9],"temperature_2m_min":[3,2],
                      "sunrise":["2026-10-18T08:31"],"sunset":["2026-10-18T17:46"]}
        }"#;
        let w = build("Berlin".into(), serde_json::from_str(json).unwrap());
        assert_eq!(w.hours.iter().map(|h| h.hour).collect::<Vec<_>>(), [12, 13]);
        assert_eq!(w.days.len(), 1);
        assert_eq!(w.days[0].dow, 1);
        assert_eq!((w.sunrise.as_str(), w.sunset.as_str()), ("08:31", "17:46"));
    }
}

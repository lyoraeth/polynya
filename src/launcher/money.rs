// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Currencies in the launcher line: "100$", "$100", "100 usd в eur", "100 долларов в рубли".
//!
//! Without a target currency it converts to the Windows region currency. Rates come from
//! open.er-api.com (free, no key, updated daily): downloaded in the background at most once a day
//! and cached in `%APPDATA%\Polynya\rates.json`.

use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use windows::{
    Win32::Globalization::{GetLocaleInfoEx, LOCALE_SINTLSYMBOL},
    core::PCWSTR,
};

/// Rates older than this are re-downloaded, s.
const MAX_AGE: u64 = 24 * 3600;

#[derive(Deserialize)]
struct Answer {
    time_last_update_unix: u64,
    /// Rates against the dollar: units of the currency per 1 USD.
    rates: HashMap<String, f64>,
}

struct Rates {
    updated: u64,
    usd: HashMap<String, f64>,
}

static RATES: Mutex<Option<Rates>> = Mutex::new(None);
static LOADED: AtomicBool = AtomicBool::new(false);
static FETCHING: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn path() -> std::path::PathBuf {
    crate::config::path().with_file_name("rates.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn store(body: &str) -> Option<u64> {
    let a: Answer = serde_json::from_str(body).ok()?;
    let updated = a.time_last_update_unix;
    *RATES.lock().ok()? = Some(Rates { updated, usd: a.rates });
    GENERATION.fetch_add(1, Ordering::Relaxed);
    Some(updated)
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Loads rates from the file and, if they are older than a day, downloads fresh ones in the background.
pub fn refresh(wake: fn()) {
    if !LOADED.swap(true, Ordering::Relaxed)
        && let Ok(body) = std::fs::read_to_string(path())
    {
        store(&body);
    }
    let fresh = RATES.lock().ok().and_then(|r| r.as_ref().map(|r| now().saturating_sub(r.updated) < MAX_AGE)).unwrap_or(false);
    if fresh || FETCHING.swap(true, Ordering::Relaxed) {
        return;
    }
    let _ = std::thread::Builder::new().name("rates".into()).spawn(move || {
        match crate::weather::get("open.er-api.com", "/v6/latest/USD") {
            Ok(body) if store(&body).is_some() => {
                let _ = std::fs::write(path(), body);
                wake();
            }
            Ok(_) => proto::warn!("launcher", "rates: unexpected response"),
            Err(e) => proto::warn!("launcher", "rates: {e}"),
        }
        FETCHING.store(false, Ordering::Relaxed);
    });
}

/// Windows region currency: the target when none is named.
fn home() -> String {
    let mut buf = [0u16; 9];
    // SAFETY: `buf` is a writable buffer whose length is passed with the slice.
    let n = unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_SINTLSYMBOL, Some(&mut buf)) };
    if n > 1 { String::from_utf16_lossy(&buf[..n as usize - 1]) } else { "USD".into() }
}

/// Currency code by symbol, word or code.
fn currency(word: &str, known: &HashMap<String, f64>) -> Option<String> {
    let w = word.trim().to_lowercase();
    let by_symbol = match w.as_str() {
        "$" => Some("USD"),
        "€" => Some("EUR"),
        "£" => Some("GBP"),
        "¥" => Some("JPY"),
        "₽" | "р" | "р." => Some("RUB"),
        "₴" => Some("UAH"),
        "₸" => Some("KZT"),
        "₺" => Some("TRY"),
        "₹" => Some("INR"),
        "₩" => Some("KRW"),
        _ => None,
    };
    // Words match by prefix: "долларов", "доллара", "бакса" are all the dollar.
    let by_word = [
        ("долл", "USD"),
        ("бакс", "USD"),
        ("евро", "EUR"),
        ("руб", "RUB"),
        ("юан", "CNY"),
        ("фунт", "GBP"),
        ("иен", "JPY"),
        ("йен", "JPY"),
        ("тенге", "KZT"),
        ("гривн", "UAH"),
        ("лир", "TRY"),
        ("франк", "CHF"),
        ("злот", "PLN"),
        ("dollar", "USD"),
        ("euro", "EUR"),
        ("pound", "GBP"),
        ("yen", "JPY"),
        ("yuan", "CNY"),
        ("ruble", "RUB"),
        ("rouble", "RUB"),
    ]
    .iter()
    .find(|(p, _)| w.starts_with(p))
    .map(|(_, c)| *c);
    let code = by_symbol.or(by_word).map(String::from).unwrap_or_else(|| w.to_uppercase());
    known.contains_key(&code).then_some(code)
}

/// Conversion: amount, source and target currencies, result and rate date.
#[derive(Debug, PartialEq)]
pub struct Conversion {
    pub to: String,
    pub value: f64,
    /// Rate time, seconds since 1970.
    pub updated: u64,
}

/// Parses the query and converts. `None`: not a currency query, or no rates yet.
pub fn convert(query: &str) -> Option<Conversion> {
    let r = RATES.lock().ok()?;
    let rates = r.as_ref()?;
    parse(query, &rates.usd, &home()).map(|(to, value)| Conversion { to, value, updated: rates.updated })
}

/// Query → (currency, amount in it). Kept apart from the rates for tests.
fn parse(query: &str, usd: &HashMap<String, f64>, home: &str) -> Option<(String, f64)> {
    let q = query.trim();
    let start = q.find(|c: char| c.is_ascii_digit())?;
    let end = q[start..].find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).map_or(q.len(), |e| start + e);
    let amount: f64 = q[start..end].replace(',', ".").parse().ok()?;
    let prefix = q[..start].trim();
    let rest = q[end..].trim();
    // "100 usd в eur": the source currency is before the amount or the first word after it.
    let (from_word, target) = match rest.split_once([' ', '\t']) {
        Some((first, tail)) if prefix.is_empty() => (first, tail.trim()),
        _ if !prefix.is_empty() => (prefix, rest),
        _ => (rest, ""),
    };
    let from = currency(from_word, usd)?;
    let target = target.strip_prefix("в ").or_else(|| target.strip_prefix("to ")).or_else(|| target.strip_prefix("in ")).unwrap_or(target).trim();
    let to = if target.is_empty() {
        // The local currency without a target goes to dollars, dollars go to euros.
        let home = home.to_string();
        if home != from {
            home
        } else if from != "USD" {
            "USD".into()
        } else {
            "EUR".into()
        }
    } else {
        currency(target, usd)?
    };
    let value = amount / usd.get(&from)? * usd.get(&to)?;
    Some((to, value))
}

/// Amount with a thousands separator and two decimals: "8 123.45".
pub fn format(value: f64) -> String {
    let s = format!("{value:.2}");
    let (int, frac) = s.split_once('.').unwrap_or((&s, "00"));
    let digits: Vec<char> = int.trim_start_matches('-').chars().collect();
    let mut grouped = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push('\u{2009}');
        }
        grouped.push(*c);
    }
    format!("{}{grouped}.{frac}", if value < 0.0 { "-" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd() -> HashMap<String, f64> {
        [("USD", 1.0), ("EUR", 0.9), ("RUB", 90.0)].map(|(k, v)| (k.to_string(), v)).into()
    }

    #[test]
    fn forms() {
        let r = usd();
        assert_eq!(parse("100$", &r, "RUB"), Some(("RUB".into(), 9000.0)));
        assert_eq!(parse("$100", &r, "RUB"), Some(("RUB".into(), 9000.0)));
        assert_eq!(parse("100 usd в eur", &r, "RUB").map(|(c, v)| (c, v.round())), Some(("EUR".into(), 90.0)));
        assert_eq!(parse("900 рублей", &r, "RUB").map(|(c, v)| (c, v.round())), Some(("USD".into(), 10.0)));
        assert_eq!(parse("10 долларов в рубли", &r, "RUB"), Some(("RUB".into(), 900.0)));
        assert_eq!(parse("100", &r, "RUB"), None);
        assert_eq!(parse("100 штук", &r, "RUB"), None);
    }

    #[test]
    fn formatting() {
        assert_eq!(format(8123.456), "8\u{2009}123.46");
        assert_eq!(format(12.0), "12.00");
    }
}

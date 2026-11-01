// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Module images: source validation, background decoding and a small device-independent cache.
//!
//! Workers produce premultiplied BGRA pixels. The UI thread only turns them into D2D bitmaps, so
//! WIC, Base64 and SVG rendering never block drawing and are never repeated per frame.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use proto::ImageRef;
use windows::{
    Win32::{
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant,
            WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnDemand,
        },
        System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
    },
    core::GUID,
};

const MAX_SOURCE: usize = 12 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_000_000;
const CACHE_BYTES: usize = 8 * 1024 * 1024;
const RECHECK: Duration = Duration::from_secs(1);
const RETRY_FAILED: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    /// Premultiplied BGRA, four bytes per pixel.
    pub pixels: Vec<u8>,
}

impl Raster {
    fn bytes(&self) -> usize {
        self.pixels.len()
    }
}

#[derive(Clone, Debug)]
enum State {
    Loading,
    Ready(Arc<Raster>),
    Failed,
}

#[derive(Debug)]
struct Entry {
    state: State,
    used: Instant,
    checked: Instant,
    fingerprint: Option<(u64, u64)>,
}

#[derive(Default)]
struct Store {
    entries: HashMap<String, Entry>,
    bytes: usize,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);
static NOTIFY: OnceLock<fn()> = OnceLock::new();

/// Sets the UI wake-up callback used when a background decode finishes.
pub fn start(notify: fn()) {
    let _ = NOTIFY.set(notify);
}

/// Stable key for a panel's D2D bitmap cache. The raster pointer remains the real identity.
pub fn key(image: &ImageRef, max_width: u32) -> String {
    let source = match (&image.path, &image.data) {
        (Some(path), _) => format!("path:{path}"),
        (None, Some(data)) => format!("data:{:x}:{}", fnv(data.as_bytes()), data.len()),
        _ => "invalid".into(),
    };
    format!("{source}:w{max_width}")
}

/// Aspect ratio known before decoding, or the actual ratio once the raster is ready.
pub fn aspect(image: &ImageRef, raster: Option<&Raster>) -> f32 {
    if let Some(raster) = raster {
        return raster.width as f32 / raster.height.max(1) as f32;
    }
    match (image.width, image.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => w as f32 / h as f32,
        _ => 2.0,
    }
}

/// Returns a decoded image, starting a background load on the first request.
pub fn get(image: &ImageRef, max_width: u32) -> Option<Arc<Raster>> {
    let key = key(image, max_width.max(1));
    let now = Instant::now();
    let mut store = STORE.lock().ok()?;
    let store = store.get_or_insert_with(Store::default);
    if let Some(entry) = store.entries.get_mut(&key) {
        entry.used = now;
        match &entry.state {
            State::Ready(raster) => {
                if image.path.is_none() || now.duration_since(entry.checked) < RECHECK {
                    return Some(raster.clone());
                }
                let fingerprint = fingerprint(image);
                entry.checked = now;
                if fingerprint == entry.fingerprint {
                    return Some(raster.clone());
                }
                entry.state = State::Loading;
                entry.fingerprint = fingerprint;
            }
            State::Loading => return None,
            State::Failed => {
                if now.duration_since(entry.checked) < RETRY_FAILED {
                    return None;
                }
                entry.state = State::Loading;
                entry.checked = now;
            }
        }
    } else {
        store.entries.insert(key.clone(), Entry { state: State::Loading, used: now, checked: now, fingerprint: fingerprint(image) });
    }
    spawn(key, image.clone(), max_width.max(1));
    None
}

fn spawn(key: String, image: ImageRef, max_width: u32) {
    let _ = std::thread::Builder::new().name("image".into()).spawn(move || {
        let result = load_raster(&image, max_width);
        let notify = NOTIFY.get().copied();
        if let Ok(mut store) = STORE.lock() {
            let store = store.get_or_insert_with(Store::default);
            match result {
                Ok(raster) => {
                    proto::debug!("images", "decoded {}x{}, {} bytes", raster.width, raster.height, raster.bytes());
                    let raster = Arc::new(raster);
                    if let Some(entry) = store.entries.get_mut(&key) {
                        if let State::Ready(previous) = &entry.state {
                            store.bytes = store.bytes.saturating_sub(previous.bytes());
                        }
                        store.bytes += raster.bytes();
                        entry.state = State::Ready(raster);
                        entry.used = Instant::now();
                    }
                    evict(store, &key);
                }
                Err(e) => {
                    if let Some(entry) = store.entries.get_mut(&key) {
                        entry.state = State::Failed;
                        entry.checked = Instant::now();
                    }
                    proto::warn!("images", "decode: {e}");
                }
            }
        }
        if let Some(notify) = notify {
            notify();
        }
    });
}

fn evict(store: &mut Store, keep: &str) {
    while store.bytes > CACHE_BYTES {
        let oldest = store
            .entries
            .iter()
            .filter(|(key, entry)| key.as_str() != keep && matches!(entry.state, State::Ready(_)))
            .min_by_key(|(_, entry)| entry.used)
            .map(|(key, _)| key.clone());
        let Some(oldest) = oldest else { break };
        if let Some(entry) = store.entries.remove(&oldest)
            && let State::Ready(raster) = entry.state
        {
            store.bytes = store.bytes.saturating_sub(raster.bytes());
        }
    }
}

fn fingerprint(image: &ImageRef) -> Option<(u64, u64)> {
    let path = image.path.as_ref()?;
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
    Some((metadata.len(), modified))
}

fn load_raster(image: &ImageRef, max_width: u32) -> Result<Raster, String> {
    let (bytes, svg) = read_source(image)?;
    if svg { decode_svg(&bytes, max_width) } else { decode_wic(&bytes, max_width) }
}

fn read_source(image: &ImageRef) -> Result<(Vec<u8>, bool), String> {
    match (&image.path, &image.data) {
        (Some(path), None) => {
            let path = std::path::Path::new(path);
            if !path.is_absolute() {
                return Err("image path must be absolute".into());
            }
            let mut file = std::fs::File::open(path).map_err(|e| format!("open image: {e}"))?;
            let mut bytes = std::io::Read::take(&mut file, MAX_SOURCE as u64 + 1);
            let mut data = Vec::new();
            std::io::Read::read_to_end(&mut bytes, &mut data).map_err(|e| format!("read image: {e}"))?;
            if data.len() > MAX_SOURCE {
                return Err("image exceeds 12 MiB".into());
            }
            let svg = image.mime.as_deref() == Some("image/svg+xml") || path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"));
            Ok((data, svg))
        }
        (None, Some(data)) => {
            if data.len() > MAX_SOURCE * 2 {
                return Err("embedded image is too large".into());
            }
            let bytes = BASE64.decode(data).map_err(|e| format!("Base64: {e}"))?;
            if bytes.len() > MAX_SOURCE {
                return Err("decoded image exceeds 12 MiB".into());
            }
            let mime = image.mime.as_deref().ok_or("embedded image needs mime")?;
            Ok((bytes, mime == "image/svg+xml"))
        }
        _ => Err("image needs exactly one source: path or data".into()),
    }
}

fn check_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("image has zero size".into());
    }
    if width as u64 * height as u64 > MAX_PIXELS {
        return Err("image exceeds 16 megapixels".into());
    }
    Ok(())
}

fn decode_wic(bytes: &[u8], max_width: u32) -> Result<Raster, String> {
    // SAFETY: initializes COM for this worker before creating WIC objects.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().map_err(|e| format!("COM: {e}"))? };
    // SAFETY: the stream reads `bytes` in place and all WIC objects are dropped before return.
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("WIC factory: {e}"))?;
        let stream = wic.CreateStream().map_err(|e| format!("WIC stream: {e}"))?;
        stream.InitializeFromMemory(bytes).map_err(|e| format!("WIC memory: {e}"))?;
        let decoder = wic.CreateDecoderFromStream(&stream, &GUID::zeroed(), WICDecodeMetadataCacheOnDemand).map_err(|e| format!("WIC decoder: {e}"))?;
        let frame = decoder.GetFrame(0).map_err(|e| format!("WIC frame: {e}"))?;
        let (mut fw, mut fh) = (0u32, 0u32);
        frame.GetSize(&mut fw, &mut fh).map_err(|e| format!("WIC size: {e}"))?;
        check_size(fw, fh)?;
        let width = max_width.min(fw).max(1);
        let height = ((fh as u64 * width as u64) / fw as u64).max(1) as u32;
        let scaler = wic.CreateBitmapScaler().map_err(|e| format!("WIC scaler: {e}"))?;
        scaler.Initialize(&frame, width, height, WICBitmapInterpolationModeFant).map_err(|e| format!("WIC scale: {e}"))?;
        let converter = wic.CreateFormatConverter().map_err(|e| format!("WIC converter: {e}"))?;
        converter
            .Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)
            .map_err(|e| format!("WIC convert: {e}"))?;
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels).map_err(|e| format!("WIC pixels: {e}"))?;
        Ok(Raster { width, height, pixels })
    }
}

fn decode_svg(bytes: &[u8], max_width: u32) -> Result<Raster, String> {
    static FONTDB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    let fontdb = FONTDB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        // Only the embedded UI fonts: no scan of the system font catalog for every SVG consumer.
        for bytes in [
            include_bytes!("../assets/fonts/Inter-Regular.ttf").as_slice(),
            include_bytes!("../assets/fonts/Inter-Medium.ttf").as_slice(),
            include_bytes!("../assets/fonts/Inter-SemiBold.ttf").as_slice(),
        ] {
            db.load_font_data(bytes.to_vec());
        }
        Arc::new(db)
    });
    let mut fontdb = fontdb.clone();
    if bytes.windows(5).any(|window| window == b"<text") {
        // Custom families and CJK fallbacks remain available, but the catalog is loaded only for text.
        std::sync::Arc::make_mut(&mut fontdb).load_system_fonts();
    }
    let options = resvg::usvg::Options { fontdb, ..Default::default() };
    // Nested mode ignores externally linked images; scripts are not executed by the renderer.
    let tree = resvg::usvg::Tree::from_data_nested(bytes, &options).map_err(|e| format!("SVG: {e}"))?;
    let size = tree.size();
    let (fw, fh) = (size.width(), size.height());
    if !fw.is_finite() || !fh.is_finite() || fw <= 0.0 || fh <= 0.0 {
        return Err("SVG has invalid size".into());
    }
    check_size(fw.ceil() as u32, fh.ceil() as u32)?;
    // SVG is resolution-independent: rasterize at the target size, including upscaling small sources.
    let scale = max_width as f32 / fw;
    let width = (fw * scale).ceil().max(1.0) as u32;
    let height = (fh * scale).ceil().max(1.0) as u32;
    check_size(width, height)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or("SVG pixmap allocation failed")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let mut pixels = pixmap.take();
    // tiny-skia produces premultiplied RGBA; D2D expects BGRA.
    for px in pixels.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    Ok(Raster { width, height, pixels })
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Opens the original image with the Windows handler for its format.
pub fn preview(image: ImageRef) {
    let _ = std::thread::Builder::new().name("image-preview".into()).spawn(move || {
        if let Err(e) = open_preview(&image) {
            proto::warn!("images", "preview: {e}");
        }
    });
}

fn open_preview(image: &ImageRef) -> Result<(), String> {
    use windows::{
        Win32::{
            System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE},
            UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        },
        core::HSTRING,
    };
    let path = preview_path(image)?;
    // SAFETY: ShellExecute runs in a single-threaded apartment; the path outlives the call.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        ShellExecuteW(None, windows::core::w!("open"), &HSTRING::from(path.as_os_str()), None, None, SW_SHOWNORMAL);
    }
    Ok(())
}

fn preview_path(image: &ImageRef) -> Result<std::path::PathBuf, String> {
    if let Some(path) = &image.path
        && image.data.is_none()
        && std::path::Path::new(path).is_absolute()
    {
        return Ok(path.into());
    }
    let (bytes, _) = read_source(image)?;
    let extension = match image.mime.as_deref() {
        Some("image/png") => "png",
        Some("image/jpeg") => "jpg",
        Some("image/svg+xml") => "svg",
        Some("image/gif") => "gif",
        Some("image/bmp") => "bmp",
        Some("image/x-icon") | Some("image/vnd.microsoft.icon") => "ico",
        Some("image/tiff") => "tiff",
        Some("image/webp") => "webp",
        _ => return Err("embedded image needs a supported mime".into()),
    };
    let dir = std::env::temp_dir().join("Polynya").join("previews");
    std::fs::create_dir_all(&dir).map_err(|e| format!("preview folder: {e}"))?;
    let path = dir.join(format!("{:x}.{extension}", fnv(&bytes)));
    if !path.is_file() {
        std::fs::write(&path, bytes).map_err(|e| format!("preview file: {e}"))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_decodes_to_bgra() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="#ff0000"/></svg>"##;
        let raster = decode_svg(svg, 10).unwrap();
        assert_eq!((raster.width, raster.height), (10, 5));
        assert_eq!(&raster.pixels[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn small_svg_is_rasterized_at_target_size() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="#ff0000"/></svg>"##;
        let raster = decode_svg(svg, 40).unwrap();
        assert_eq!((raster.width, raster.height), (40, 40));
    }

    #[test]
    fn png_decodes_through_wic() {
        let bytes = BASE64.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap();
        let raster = decode_wic(&bytes, 64).unwrap();
        assert_eq!((raster.width, raster.height), (1, 1));
        assert_eq!(raster.pixels.len(), 4);
    }

    #[test]
    fn source_requires_exactly_one_input() {
        let image = ImageRef { path: Some("relative.png".into()), data: None, mime: None, alt: String::new(), preview: false, width: None, height: None };
        assert!(read_source(&image).is_err());
        let image =
            ImageRef { path: None, data: Some("%%%".into()), mime: Some("image/png".into()), alt: String::new(), preview: false, width: None, height: None };
        assert!(read_source(&image).is_err());
    }

    #[test]
    fn aspect_uses_hints_before_loading() {
        let image = ImageRef { path: None, data: None, mime: None, alt: String::new(), preview: false, width: Some(800), height: Some(400) };
        assert_eq!(aspect(&image, None), 2.0);
    }
}

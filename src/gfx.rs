// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Window graphics stack: D3D11 → Direct2D → DirectComposition surface.
//!
//! The window has no surface of its own (`WS_EX_NOREDIRECTIONBITMAP`): DWM composes our visual
//! directly, with per-pixel alpha. Drawing happens only on events; no frames are produced at rest.
//!
//! The picture goes to the system as a DirectComposition surface, not a swap chain: on Intel
//! integrated graphics the driver sets aside about 50 MB of internal memory for the first swap chain
//! regardless of its size, while a composition surface costs a few megabytes, with the same drawing
//! and slightly less CPU.

use windows::{
    Win32::{
        Foundation::{D2DERR_RECREATE_TARGET, HMODULE, HWND, POINT},
        Graphics::{
            Direct2D::{
                CLSID_D2D1GaussianBlur,
                Common::{
                    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BORDER_MODE_HARD, D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER,
                    D2D1_PIXEL_FORMAT,
                },
                D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_CAP_STYLE_ROUND,
                D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_OPTIONS, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAUSSIANBLUR_PROP_BORDER_MODE,
                D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LINE_JOIN_ROUND, D2D1_PROPERTY_TYPE_ENUM,
                D2D1_PROPERTY_TYPE_FLOAT, D2D1_STROKE_STYLE_PROPERTIES1, D2D1CreateFactory, ID2D1DeviceContext, ID2D1Factory1, ID2D1StrokeStyle1,
            },
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN},
            Direct3D11::{D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device},
            DirectComposition::{DCompositionCreateDevice, IDCompositionDevice, IDCompositionSurface, IDCompositionTarget, IDCompositionVisual},
            DirectWrite::{DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory},
            Dwm::DwmFlush,
            Dxgi::{
                Common::{DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM},
                CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET, DXGI_GPU_PREFERENCE_MINIMUM_POWER,
                IDXGIAdapter, IDXGIAdapter1, IDXGIDevice, IDXGIFactory2, IDXGIFactory6, IDXGISurface,
            },
            Imaging::{
                CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut,
                WICDecodeMetadataCacheOnDemand,
            },
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    },
    core::{GUID, Interface, Result},
};
use windows_numerics::Matrix3x2;

use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;

/// Creates the D3D11 device on the most power-efficient adapter.
///
/// DWM composes the screen on the GPU the display is connected to, which on laptops is the
/// integrated one. Drawing on the discrete GPU would mean copying every frame between GPUs and
/// waking the discrete GPU for a couple of bars. If no adapter can be chosen, the system default
/// remains.
fn create_device(factory: &IDXGIFactory2) -> Result<ID3D11Device> {
    // SAFETY: `d3d` is a valid out slot, and `adapter` is a live COM object or None.
    let create = |adapter: Option<&IDXGIAdapter>| unsafe {
        let mut d3d: Option<ID3D11Device> = None;
        let driver = if adapter.is_some() { D3D_DRIVER_TYPE_UNKNOWN } else { D3D_DRIVER_TYPE_HARDWARE };
        D3D11CreateDevice(adapter, driver, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut d3d), None, None)
            .map(|_| d3d.unwrap())
    };
    let preferred = factory
        .cast::<IDXGIFactory6>()
        // SAFETY: COM call on a live factory.
        .and_then(|f6| unsafe { f6.EnumAdapterByGpuPreference::<IDXGIAdapter1>(0, DXGI_GPU_PREFERENCE_MINIMUM_POWER) });
    if let Ok(adapter) = preferred {
        // SAFETY: COM call on a live adapter.
        if let Ok(desc) = unsafe { adapter.GetDesc1() } {
            let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
            proto::info!("gfx", "adapter: {}", String::from_utf16_lossy(&desc.Description[..len]));
        }
        if let Ok(d3d) = adapter.cast::<IDXGIAdapter>().and_then(|a| create(Some(&a))) {
            return Ok(d3d);
        }
    }
    create(None)
}

pub struct Gfx {
    pub d2d: ID2D1Factory1,
    pub dwrite: IDWriteFactory,
    pub dc: ID2D1DeviceContext,
    /// Round-capped stroke style for icons; created once.
    pub round: ID2D1StrokeStyle1,
    wic: IWICImagingFactory,
    /// Surface covering the whole window, physical pixels.
    surface: IDCompositionSurface,
    size: (u32, u32),
    pub dcomp: IDCompositionDevice,
    /// Tree root: its content is the main surface, its children are layers on top of it.
    pub root: IDCompositionVisual,
    /// Keeps the visual tree alive; unused after creation.
    _target: IDCompositionTarget,
    /// Second drawing context, for layers and images prepared mid-frame.
    offscreen: std::cell::RefCell<Option<ID2D1DeviceContext>>,
    pub dpi: f32,
}

impl Gfx {
    /// Surface height, physical pixels.
    pub fn height(&self) -> u32 {
        self.size.1
    }

    /// Grows the surface to `h` pixels: the new surface becomes the root's content. The frame is
    /// always drawn in full, so the old content need not be carried over.
    pub fn grow(&mut self, h: u32) -> Result<()> {
        if h <= self.size.1 {
            return Ok(());
        }
        // SAFETY: COM calls on live objects owned by `self`; every argument outlives its call.
        unsafe {
            let surface = self.dcomp.CreateSurface(self.size.0, h, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_PREMULTIPLIED)?;
            self.root.SetContent(&surface)?;
            self.surface = surface;
        }
        self.size.1 = h;
        Ok(())
    }

    /// Creates the graphics stack for a window of `w`×`h` physical pixels.
    pub fn new(hwnd: HWND, w: u32, h: u32, dpi: f32) -> Result<Self> {
        // SAFETY: COM and DirectX factory calls with arguments that outlive them; `hwnd` is our window.
        unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let d3d = create_device(&factory)?;
            let dxgi: IDXGIDevice = d3d.cast()?;

            let d2d: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&D2D1_FACTORY_OPTIONS::default()))?;
            let dc = d2d.CreateDevice(&dxgi)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            dc.SetDpi(dpi, dpi);
            let round = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES1 {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    ..Default::default()
                },
                None,
            )?;

            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let surface = dcomp.CreateSurface(w, h, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_PREMULTIPLIED)?;
            let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&surface)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;

            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            Ok(Self { d2d, dwrite, dc, round, wic, surface, size: (w, h), dcomp, root: visual, _target: target, offscreen: Default::default(), dpi })
        }
    }

    /// Whether the error means a lost device (sleep, driver update or crash): the whole graphics
    /// stack must then be recreated.
    pub fn is_device_lost(e: &windows::core::Error) -> bool {
        let code = e.code();
        code == D2DERR_RECREATE_TARGET || code == DXGI_ERROR_DEVICE_REMOVED || code == DXGI_ERROR_DEVICE_RESET
    }

    /// A picture from an `HBITMAP` with alpha: shell icons. The `HBITMAP` stays with the caller.
    pub fn bitmap_from_hbitmap(&self, hbm: windows::Win32::Graphics::Gdi::HBITMAP) -> Result<ID2D1Bitmap1> {
        // SAFETY: COM calls on live WIC/D2D objects; `hbm` is a valid bitmap owned by the caller.
        unsafe {
            let src = self.wic.CreateBitmapFromHBITMAP(
                hbm,
                windows::Win32::Graphics::Gdi::HPALETTE::default(),
                windows::Win32::Graphics::Imaging::WICBitmapUseAlpha,
            )?;
            let converter = self.wic.CreateFormatConverter()?;
            converter.Initialize(&src, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
            self.dc.CreateBitmapFromWicBitmap(&converter, None)
        }
    }

    /// Blurred copy of a picture at `scale` (blur `sigma` is at the source scale).
    ///
    /// Computed once and then drawn ready: a blur effect on every animation frame would be the most
    /// expensive thing in the frame. A separate context draws it: the main one may be mid-frame, and
    /// the target is not changed mid-frame.
    pub fn blurred(&self, src: &ID2D1Bitmap1, sigma: f32, scale: f32) -> Result<ID2D1Bitmap1> {
        // SAFETY: COM calls on live objects owned by `self`; `BeginDraw` and `EndDraw` are paired on
        // every path that reaches them.
        unsafe {
            let dc = self.offscreen()?;
            let size = src.GetPixelSize();
            let px = D2D_SIZE_U { width: ((size.width as f32 * scale).ceil() as u32).max(1), height: ((size.height as f32 * scale).ceil() as u32).max(1) };
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: self.dpi,
                dpiY: self.dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                ..Default::default()
            };
            let out = dc.CreateBitmap(px, None, 0, &props)?;
            let blur = dc.CreateEffect(&CLSID_D2D1GaussianBlur)?;
            blur.SetInput(0, src, true);
            blur.SetValue(D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &sigma.to_le_bytes())?;
            // Hard border, otherwise blur edges fade to transparent and a frame shows.
            blur.SetValue(D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 as u32, D2D1_PROPERTY_TYPE_ENUM, &(D2D1_BORDER_MODE_HARD.0 as u32).to_le_bytes())?;
            dc.SetTarget(&out);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            dc.SetTransform(&Matrix3x2 { M11: scale, M12: 0.0, M21: 0.0, M22: scale, M31: 0.0, M32: 0.0 });
            dc.DrawImage(&blur.GetOutput()?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            dc.EndDraw(None, None)?;
            dc.SetTarget(None);
            dc.SetTransform(&Matrix3x2::identity());
            Ok(out)
        }
    }

    /// A `w`×`h` DIP picture drawn by `draw` and distorted by Perlin noise: pixels shift along the
    /// turbulence field by `shift` DIPs, with a noise wavelength around `1 / freq` DIPs. `draw` paints
    /// with a `shift` margin around, since the shift also pulls pixels from beyond the edge.
    ///
    /// Computed once and then drawn ready: no per-frame effects.
    pub fn turbulent(&self, w: f32, h: f32, shift: f32, freq: f32, draw: impl FnOnce(&ID2D1DeviceContext) -> Result<()>) -> Result<ID2D1Bitmap1> {
        use windows::Win32::Graphics::Direct2D::{
            CLSID_D2D1DisplacementMap, CLSID_D2D1Turbulence, D2D1_DISPLACEMENTMAP_PROP_SCALE, D2D1_DISPLACEMENTMAP_PROP_X_CHANNEL_SELECT,
            D2D1_DISPLACEMENTMAP_PROP_Y_CHANNEL_SELECT, D2D1_PROPERTY_TYPE_UINT32, D2D1_PROPERTY_TYPE_VECTOR2, D2D1_TURBULENCE_PROP_BASE_FREQUENCY,
            D2D1_TURBULENCE_PROP_NUM_OCTAVES, D2D1_TURBULENCE_PROP_SIZE,
        };
        // SAFETY: COM calls on live objects owned by `self`; `BeginDraw` and `EndDraw` are paired on
        // every path that reaches them.
        unsafe {
            let dc = self.offscreen()?;
            let k = self.dpi / 96.0;
            let target = |w: f32, h: f32| {
                let props = D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                    dpiX: self.dpi,
                    dpiY: self.dpi,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                    ..Default::default()
                };
                dc.CreateBitmap(D2D_SIZE_U { width: ((w * k).ceil() as u32).max(1), height: ((h * k).ceil() as u32).max(1) }, None, 0, &props)
            };
            let (pw, ph) = (w + shift * 2.0, h + shift * 2.0);
            let src = target(pw, ph)?;
            dc.SetTarget(&src);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            let drawn = draw(&dc);
            dc.EndDraw(None, None)?;
            drawn?;
            let v2 = |x: f32, y: f32| [x.to_le_bytes(), y.to_le_bytes()].concat();
            let noise = dc.CreateEffect(&CLSID_D2D1Turbulence)?;
            noise.SetValue(D2D1_TURBULENCE_PROP_SIZE.0 as u32, D2D1_PROPERTY_TYPE_VECTOR2, &v2(pw, ph))?;
            noise.SetValue(D2D1_TURBULENCE_PROP_BASE_FREQUENCY.0 as u32, D2D1_PROPERTY_TYPE_VECTOR2, &v2(freq, freq))?;
            noise.SetValue(D2D1_TURBULENCE_PROP_NUM_OCTAVES.0 as u32, D2D1_PROPERTY_TYPE_UINT32, &2u32.to_le_bytes())?;
            let warp = dc.CreateEffect(&CLSID_D2D1DisplacementMap)?;
            warp.SetInput(0, &src, true);
            warp.SetInput(1, &noise.GetOutput()?, true);
            warp.SetValue(D2D1_DISPLACEMENTMAP_PROP_SCALE.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &(shift * 2.0).to_le_bytes())?;
            // The noise's red and green channels are the x and y shift.
            warp.SetValue(D2D1_DISPLACEMENTMAP_PROP_X_CHANNEL_SELECT.0 as u32, D2D1_PROPERTY_TYPE_ENUM, &0u32.to_le_bytes())?;
            warp.SetValue(D2D1_DISPLACEMENTMAP_PROP_Y_CHANNEL_SELECT.0 as u32, D2D1_PROPERTY_TYPE_ENUM, &1u32.to_le_bytes())?;
            let out = target(w, h)?;
            dc.SetTarget(&out);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            // The margin is cut off.
            dc.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: -shift, M32: -shift });
            dc.DrawImage(&warp.GetOutput()?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            dc.EndDraw(None, None)?;
            dc.SetTarget(None);
            dc.SetTransform(&Matrix3x2::identity());
            Ok(out)
        }
    }

    /// Blurred shadow of a shape drawn by `draw` on a `w`×`h` DIP picture with a `pad` margin:
    /// the result is `(w + 2·pad)`×`(h + 2·pad)`, with the shape offset by `pad`.
    pub fn shadow(&self, w: f32, h: f32, pad: f32, sigma: f32, draw: impl FnOnce(&ID2D1DeviceContext) -> Result<()>) -> Result<ID2D1Bitmap1> {
        use windows::Win32::Graphics::Direct2D::{CLSID_D2D1Shadow, D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION};
        // SAFETY: COM calls on live objects owned by `self`; `BeginDraw` and `EndDraw` are paired on
        // every path that reaches them.
        unsafe {
            let dc = self.offscreen()?;
            let k = self.dpi / 96.0;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: self.dpi,
                dpiY: self.dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                ..Default::default()
            };
            let size = D2D_SIZE_U { width: (((w + pad * 2.0) * k).ceil() as u32).max(1), height: (((h + pad * 2.0) * k).ceil() as u32).max(1) };
            let src = dc.CreateBitmap(size, None, 0, &props)?;
            dc.SetTarget(&src);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            dc.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: pad, M32: pad });
            let drawn = draw(&dc);
            dc.SetTransform(&Matrix3x2::identity());
            dc.EndDraw(None, None)?;
            drawn?;
            let shadow = dc.CreateEffect(&CLSID_D2D1Shadow)?;
            shadow.SetInput(0, &src, true);
            shadow.SetValue(D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &sigma.to_le_bytes())?;
            let out = dc.CreateBitmap(size, None, 0, &props)?;
            dc.SetTarget(&out);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            dc.DrawImage(&shadow.GetOutput()?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            dc.EndDraw(None, None)?;
            dc.SetTarget(None);
            Ok(out)
        }
    }

    /// Prewarms the dropdown vignette and shadow effects: an effect's first run compiles its shaders
    /// (hundreds of ms), and without prewarming that would happen on the first dropdown open as a
    /// visible stutter. The blur needs no prewarm: the glass uses it at startup.
    pub fn warm(&self) -> Result<()> {
        self.turbulent(16.0, 16.0, 2.0, 0.1, |_| Ok(()))?;
        self.shadow(16.0, 16.0, 4.0, 2.0, |_| Ok(()))?;
        Ok(())
    }

    fn offscreen(&self) -> Result<ID2D1DeviceContext> {
        let mut slot = self.offscreen.borrow_mut();
        if let Some(dc) = slot.as_ref() {
            return Ok(dc.clone());
        }
        // SAFETY: COM calls on live objects owned by `self`; every argument outlives its call.
        let dc = unsafe { self.dc.GetDevice()?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)? };
        // SAFETY: COM calls on live objects owned by `self`; every argument outlives its call.
        unsafe { dc.SetDpi(self.dpi, self.dpi) };
        *slot = Some(dc.clone());
        Ok(dc)
    }

    /// Composition surface for a `w`×`h` DIP layer; returns it and its size in pixels.
    pub fn layer_surface(&self, w: f32, h: f32) -> Result<(IDCompositionSurface, (u32, u32))> {
        let k = self.dpi / 96.0;
        let px = (((w * k).ceil() as u32).max(1), ((h * k).ceil() as u32).max(1));
        // SAFETY: COM calls on live objects owned by `self`; every argument outlives its call.
        Ok((unsafe { self.dcomp.CreateSurface(px.0, px.1, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_PREMULTIPLIED)? }, px))
    }

    /// Draws a layer's content into its surface; `draw` gets a context in DIPs from the layer's top
    /// left corner. Safe mid-frame: a separate context draws it.
    pub fn paint(&self, surface: &IDCompositionSurface, px: (u32, u32), draw: impl FnOnce(&ID2D1DeviceContext)) -> Result<()> {
        // SAFETY: COM calls on live objects; the surface's `BeginDraw`/`EndDraw` and the context's
        // `BeginDraw`/`EndDraw` are paired on every path past them.
        unsafe {
            let dc = self.offscreen()?;
            let mut offset = POINT::default();
            let dxgi: IDXGISurface = surface.BeginDraw(None, &mut offset)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: self.dpi,
                dpiY: self.dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let target = dc.CreateBitmapFromDxgiSurface(&dxgi, Some(&props))?;
            dc.SetTarget(&target);
            dc.BeginDraw();
            let dip = 96.0 / self.dpi;
            dc.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: offset.x as f32 * dip, M32: offset.y as f32 * dip });
            dc.PushAxisAlignedClip(&D2D_RECT_F { left: 0.0, top: 0.0, right: px.0 as f32 * dip, bottom: px.1 as f32 * dip }, D2D1_ANTIALIAS_MODE_ALIASED);
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            draw(&dc);
            dc.PopAxisAlignedClip();
            dc.SetTransform(&Matrix3x2::identity());
            let end = dc.EndDraw(None, None);
            dc.SetTarget(None);
            surface.EndDraw()?;
            end
        }
    }

    pub fn decode(&self, bytes: &[u8]) -> Result<ID2D1Bitmap1> {
        // SAFETY: the WIC stream reads `bytes` in place, and every WIC object is dropped before return,
        // so `bytes` outlives them all.
        unsafe {
            let stream = self.wic.CreateStream()?;
            stream.InitializeFromMemory(bytes)?;
            let decoder = self.wic.CreateDecoderFromStream(&stream, &GUID::zeroed(), WICDecodeMetadataCacheOnDemand)?;
            let frame = decoder.GetFrame(0)?;
            let converter = self.wic.CreateFormatConverter()?;
            converter.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
            self.dc.CreateBitmapFromWicBitmap(&converter, None)
        }
    }

    /// Decodes a picture already scaled down to width `w` (keeping proportions): the blurred background
    /// does not need the full size, which would cost tens of megabytes.
    pub fn decode_scaled(&self, bytes: &[u8], w: u32) -> Result<ID2D1Bitmap1> {
        // SAFETY: the WIC stream reads `bytes` in place, and every WIC object is dropped before return,
        // so `bytes` outlives them all.
        unsafe {
            let stream = self.wic.CreateStream()?;
            stream.InitializeFromMemory(bytes)?;
            let decoder = self.wic.CreateDecoderFromStream(&stream, &GUID::zeroed(), WICDecodeMetadataCacheOnDemand)?;
            let frame = decoder.GetFrame(0)?;
            let (mut fw, mut fh) = (0u32, 0u32);
            frame.GetSize(&mut fw, &mut fh)?;
            let w = w.min(fw).max(1);
            let h = ((fh as u64 * w as u64) / fw.max(1) as u64).max(1) as u32;
            let scaler = self.wic.CreateBitmapScaler()?;
            scaler.Initialize(&frame, w, h, windows::Win32::Graphics::Imaging::WICBitmapInterpolationModeFant)?;
            let converter = self.wic.CreateFormatConverter()?;
            converter.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
            self.dc.CreateBitmapFromWicBitmap(&converter, None)
        }
    }

    /// Releases Direct2D caches after one-off heavy work (wallpaper blur).
    pub fn trim(&self) {
        // SAFETY: COM calls on live objects owned by `self`; every argument outlives its call.
        unsafe {
            if let Ok(d) = self.dc.GetDevice() {
                d.ClearResources(0);
            }
        }
    }

    /// Average color of part of a picture (0..1): `part` is width and height fractions `(l, t, r, b)`.
    /// WIC crops the part and scales it down to one pixel.
    pub fn average(&self, bytes: &[u8], part: (f32, f32, f32, f32)) -> Result<[f32; 3]> {
        // SAFETY: the WIC stream reads `bytes` in place, and every WIC object is dropped before return,
        // so `bytes` outlives them all.
        unsafe {
            let stream = self.wic.CreateStream()?;
            stream.InitializeFromMemory(bytes)?;
            let decoder = self.wic.CreateDecoderFromStream(&stream, &GUID::zeroed(), WICDecodeMetadataCacheOnDemand)?;
            let frame = decoder.GetFrame(0)?;
            let (mut w, mut h) = (0u32, 0u32);
            frame.GetSize(&mut w, &mut h)?;
            let px = |f: f32, n: u32| ((f.clamp(0.0, 1.0) * n as f32) as i32).min(n as i32 - 1);
            let (l, t) = (px(part.0, w), px(part.1, h));
            let rect = windows::Win32::Graphics::Imaging::WICRect { X: l, Y: t, Width: (px(part.2, w) - l).max(1), Height: (px(part.3, h) - t).max(1) };
            let clip = self.wic.CreateBitmapClipper()?;
            clip.Initialize(&frame, &rect)?;
            let scaler = self.wic.CreateBitmapScaler()?;
            scaler.Initialize(&clip, 1, 1, windows::Win32::Graphics::Imaging::WICBitmapInterpolationModeFant)?;
            let converter = self.wic.CreateFormatConverter()?;
            converter.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
            let mut px = [0u8; 4];
            converter.CopyPixels(std::ptr::null(), 4, &mut px)?;
            Ok([px[2] as f32 / 255.0, px[1] as f32 / 255.0, px[0] as f32 / 255.0])
        }
    }

    /// Draws one frame of the main surface: clears it to transparent and calls `draw`.
    /// The frame reaches the screen together with the layers on [`Gfx::present`].
    pub fn frame(&self, draw: impl FnOnce(&ID2D1DeviceContext)) -> Result<()> {
        // SAFETY: COM calls on live objects owned by `self`; the surface's and the context's
        // `BeginDraw`/`EndDraw` are paired on every path past them.
        unsafe {
            // The surface may lie inside a shared composition texture, at an offset.
            let mut offset = POINT::default();
            let surface: IDXGISurface = self.surface.BeginDraw(None, &mut offset)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: self.dpi,
                dpiY: self.dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let target = self.dc.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
            self.dc.SetTarget(&target);
            self.dc.BeginDraw();
            let dip = 96.0 / self.dpi;
            self.dc.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: offset.x as f32 * dip, M32: offset.y as f32 * dip });
            // Outside our area in the shared texture are other surfaces; leave them alone.
            let area = D2D_RECT_F { left: 0.0, top: 0.0, right: self.size.0 as f32 * dip, bottom: self.size.1 as f32 * dip };
            self.dc.PushAxisAlignedClip(&area, D2D1_ANTIALIAS_MODE_ALIASED);
            self.dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            draw(&self.dc);
            self.dc.PopAxisAlignedClip();
            self.dc.SetTransform(&Matrix3x2::identity());
            let end = self.dc.EndDraw(None, None);
            self.dc.SetTarget(None);
            self.surface.EndDraw()?;
            end
        }
    }

    /// Hands everything drawn to composition (the frame and layers in one change) and waits until DWM
    /// shows it. The wait paces animations at no more than the refresh rate; the caller paces background
    /// frames.
    pub fn present(&self) -> Result<()> {
        // SAFETY: COM calls on live objects owned by `self`; every argument outlives its call.
        unsafe {
            self.dcomp.Commit()?;
            DwmFlush()
        }
    }
}

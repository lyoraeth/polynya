// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Island: segment layout, shape, springs, hover, and reveal choreography.
//!
//! Content lives in components: pill segments, right-zone chips, and expandable panels.
//! The island orchestrates them: arranges by width, tracks hover, grows the shape,
//! and passes the frame context to each. All coordinates are in DIPs relative to the window.

mod access;
mod field;
pub mod kit;
mod logo;
mod panels;
mod scroller;
mod segments;
mod tone;
pub mod zone;

use windows::{
    Win32::Graphics::{
        Direct2D::{
            Common::{D2D_RECT_F, D2D_SIZE_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED},
            D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_ROUNDED_RECT,
            D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, ID2D1DeviceContext, ID2D1Geometry, ID2D1PathGeometry,
        },
        DirectWrite::IDWriteFactory,
    },
    core::{Interface, Result},
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::{
    gfx::Gfx,
    media::{self, Cmd},
    shape,
    spring::{Params, Spring},
    weather,
};
use kit::{Brushes, Frame, HotKind, Hotspot, Rect, Texts, ease_in_out, inside, tween};
pub use panels::MenuEffect;
use panels::{
    Area, CONTENT_DUR, CONTENT_STAGGER, CONTENT_START, LauncherPanel, MenuPanel, NotePanel, PanelView, PlayerPanel, Reveal, STACK_ROWS, STAGGER_ROWS,
    StackPanel, WeatherPanel,
};
use segments::{Data, Hover, Segment};
use zone::{Note, Zone};

pub use access::{Role, Snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Time,
    Player,
    Weather,
    Note(u32),
    More,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Player,
    Weather,
    Note,
    Stack,
    Launcher,
    Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Part(Part),
    Pill,
    Panel,
    None,
}

#[derive(Debug, Clone, Copy)]
pub enum Action {
    None,
    Open(Panel, Part),
    Media(Cmd),
    NotificationCenter,
    Dismiss,
    Launch(usize),
    LaunchAct(usize),
    Swatch(usize),
    OpenApp(Part),
    MenuRow(usize),
    MenuBack,
    Close,
}

const POPUP_TTL: std::time::Duration = std::time::Duration::from_secs(5);

const ISLAND_W: f32 = 1100.0;

const BORDER_MIN_DL: f32 = 0.02;
const BORDER_TAIL: f32 = 2.0;
const EDGE_FADE: f32 = 56.0;
const VIGNETTE: (f32, f32) = (0.25, 0.6);
const VIGNETTE_SIZE: f32 = 0.6;
const VIGNETTE_SHIFT: f32 = 24.0;
const VIGNETTE_WAVE: f32 = 90.0;
const SEAM_SHARE: f32 = 0.35;

fn draw_plate(dc: &ID2D1DeviceContext, surface: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush, (l, t, r, b): Rect, hov: f32) {
    let radius = (b - t) / 2.0;
    let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: l, top: t, right: r, bottom: b }, radiusX: radius, radiusY: radius };
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        dc.FillRoundedRectangle(&rr, surface);
        if hov > 0.0 {
            surface.SetOpacity(hov * HOVER_DENSER);
            dc.FillRoundedRectangle(&rr, surface);
            surface.SetOpacity(1.0);
        }
    }
}

const HOVER_DENSER: f32 = 0.6;

const NOISE_MIN: f32 = 0.002;
const TILE: u32 = 128;
const TILE_DIFF: f32 = 0.083;
const SHADOW_SIGMA: f32 = 6.0;
const SHADOW_PAD: f32 = 18.0;
const SHADOW_ALPHA: f32 = 0.4;

fn dither_tile(gfx: &Gfx, dc: &ID2D1DeviceContext) -> Result<windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush> {
    use windows::Win32::Graphics::{
        Direct2D::{
            Common::{D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT},
            D2D1_BITMAP_BRUSH_PROPERTIES1, D2D1_BITMAP_PROPERTIES1, D2D1_EXTEND_MODE_WRAP, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
        },
        Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
    };
    let mut s = 0x9e37_79b9u32;
    let mut px = Vec::with_capacity((TILE * TILE * 4) as usize);
    for _ in 0..TILE * TILE {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        let m = (s & 0x3f) as u8;
        let v = if s & 0x100 != 0 { m } else { 0 };
        px.extend_from_slice(&[v, v, v, m]);
    }
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: gfx.dpi,
            dpiY: gfx.dpi,
            ..Default::default()
        };
        let bmp = dc.CreateBitmap(D2D_SIZE_U { width: TILE, height: TILE }, Some(px.as_ptr().cast()), TILE * 4, &props)?;
        let brush = dc.CreateBitmapBrush(
            &bmp,
            Some(&D2D1_BITMAP_BRUSH_PROPERTIES1 {
                extendModeX: D2D1_EXTEND_MODE_WRAP,
                extendModeY: D2D1_EXTEND_MODE_WRAP,
                interpolationMode: D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
            }),
            None,
        )?;
        brush.cast()
    }
}

fn bake_shadow(gfx: &Gfx, dc: &ID2D1DeviceContext, (w, d): (f32, f32)) -> Result<windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush> {
    let f = shape::radii(w, d).0;
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    let bmp = gfx.shadow(w + f * 2.0, d, SHADOW_PAD, SHADOW_SIGMA, |odc| unsafe {
        if let Some(o) = shape::flow(f + w / 2.0, w, d, 0.0, 0.0) {
            let geo = outline_geometry(gfx, &o, true)?;
            let black = odc.CreateSolidColorBrush(&kit::black(1.0), None)?;
            odc.FillGeometry(&geo, &black, None);
        }
        Ok(())
    })?;
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        let props = windows::Win32::Graphics::Direct2D::D2D1_BITMAP_BRUSH_PROPERTIES1 {
            extendModeX: windows::Win32::Graphics::Direct2D::D2D1_EXTEND_MODE_CLAMP,
            extendModeY: windows::Win32::Graphics::Direct2D::D2D1_EXTEND_MODE_CLAMP,
            interpolationMode: windows::Win32::Graphics::Direct2D::D2D1_INTERPOLATION_MODE_LINEAR,
        };
        dc.CreateBitmapBrush(&bmp, Some(&props), None)?.cast()
    }
}

fn draw_shadow(
    dc: &ID2D1DeviceContext,
    shadow: &windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush,
    (cx, w, d): (f32, f32, f32),
    (fw, fd): (f32, f32),
    edge: f32,
    bottom: bool,
    width: f32,
) {
    let (sx, sy) = (w / fw.max(1.0), d / fd.max(1.0));
    let ff = shape::radii(fw, fd).0;
    let x0 = cx - (ff + fw / 2.0 + SHADOW_PAD) * sx;
    let (m22, y0) = if bottom { (-sy, edge + SHADOW_PAD * sy) } else { (sy, edge - SHADOW_PAD * sy) };
    let side = if bottom {
        D2D_RECT_F { left: 0.0, top: edge - d - SHADOW_PAD, right: width, bottom: edge }
    } else {
        D2D_RECT_F { left: 0.0, top: edge, right: width, bottom: edge + d + SHADOW_PAD }
    };
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        shadow.SetTransform(&Matrix3x2 { M11: sx, M12: 0.0, M21: 0.0, M22: m22, M31: x0, M32: y0 });
        shadow.SetOpacity(SHADOW_ALPHA);
        dc.PushAxisAlignedClip(&side, windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED);
        dc.FillRectangle(&side, shadow);
        dc.PopAxisAlignedClip();
    }
}

fn bake_vignette(gfx: &Gfx, dc: &ID2D1DeviceContext, size: (f32, f32), rim: f32) -> Result<windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush> {
    let (w, h) = size;
    let s = VIGNETTE_SHIFT;
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    let bmp = gfx.turbulent(w, h, s, 1.0 / VIGNETTE_WAVE, |odc| unsafe {
        let v = kit::vignette(odc, s + w / 2.0, s + h / 2.0, w * VIGNETTE_SIZE, h * VIGNETTE_SIZE, rim)?;
        odc.FillRectangle(&D2D_RECT_F { left: 0.0, top: 0.0, right: w + s * 2.0, bottom: h + s * 2.0 }, &v);
        Ok(())
    })?;
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        let props = windows::Win32::Graphics::Direct2D::D2D1_BITMAP_BRUSH_PROPERTIES1 {
            extendModeX: windows::Win32::Graphics::Direct2D::D2D1_EXTEND_MODE_CLAMP,
            extendModeY: windows::Win32::Graphics::Direct2D::D2D1_EXTEND_MODE_CLAMP,
            interpolationMode: windows::Win32::Graphics::Direct2D::D2D1_INTERPOLATION_MODE_LINEAR,
        };
        dc.CreateBitmapBrush(&bmp, Some(&props), None)?.cast()
    }
}
const TINT_MAX: f32 = 0.65;

fn edge_brush(gfx: &Gfx, dc: &ID2D1DeviceContext, cols: &[crate::look::Rgb], x: f32) -> Result<windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush> {
    use windows::Win32::Graphics::{
        Direct2D::{
            Common::{D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT},
            D2D1_BITMAP_BRUSH_PROPERTIES1, D2D1_BITMAP_PROPERTIES1, D2D1_EXTEND_MODE_CLAMP, D2D1_INTERPOLATION_MODE_LINEAR,
        },
        Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
    };
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let px: Vec<u8> = cols.iter().flat_map(|c| [b(c[2]), b(c[1]), b(c[0]), 255]).collect();
    // SAFETY: `px` holds `cols.len()` BGRA pixels, the exact size and pitch passed, and outlives
    // `CreateBitmap`, which copies it.
    unsafe {
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: gfx.dpi,
            dpiY: gfx.dpi,
            ..Default::default()
        };
        let bmp = dc.CreateBitmap(D2D_SIZE_U { width: cols.len() as u32, height: 1 }, Some(px.as_ptr().cast()), (cols.len() * 4) as u32, &props)?;
        let brush = dc.CreateBitmapBrush(
            &bmp,
            Some(&D2D1_BITMAP_BRUSH_PROPERTIES1 {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_INTERPOLATION_MODE_LINEAR,
            }),
            None,
        )?;
        brush.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: x, M32: 0.0 });
        brush.cast()
    }
}

fn fill_flow(
    dc: &ID2D1DeviceContext,
    body_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    tint: f32,
    outline: &windows::Win32::Graphics::Direct2D::ID2D1PathGeometry,
    glass: Option<&windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush>,
) {
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        match glass {
            Some(g) => {
                dc.FillGeometry(outline, g, None);
                kit::set_opacity(body_brush, tint);
            }
            None => kit::set_opacity(body_brush, 1.0),
        }
        dc.FillGeometry(outline, body_brush, None);
        kit::set_opacity(body_brush, 1.0);
    }
}

const FREE_PAD: f32 = 8.0;

const INSET: f32 = 4.0;
const BLOCK_PAD: f32 = 12.0;
const BLOCK_GAP: f32 = 8.0;
const BLOCK_INNER: f32 = 10.0;
const DIVIDER_W: f32 = 1.0;
const DIVIDER_H: f32 = 12.0;
const DIVIDER_ALPHA: f32 = 0.35;
const ZONE_EDGE: f32 = 8.0;
const ANCHOR: Part = Part::Time;
const SEARCH_ICON: f32 = 15.0;
const HOVER_DELAY: f32 = 0.09;
const HOVER_FADE: f32 = 0.18;

const OPEN_W: Params = Params { k: 300.0, c: 26.0 };
const OPEN_D: Params = Params { k: 230.0, c: 19.0 };
const CLOSE: Params = Params { k: 420.0, c: 42.0 };
const OPEN_D_DELAY: f32 = 0.035;
const CLOSE_DELAY: f32 = 0.06;

#[derive(Clone, Copy)]
enum Pending {
    Depth(f32),
    Retract { cx: f32, w: f32 },
}

pub struct Island {
    pub h: f32,
    bottom: bool,
    win_h: f32,
    glass: Option<windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush>,
    looked: bool,
    edge: Option<(Vec<crate::look::Rgb>, f32)>,
    edge_brush: Option<windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush>,
    border: Vec<crate::look::Rgb>,
    /// Taskbar outline thickness in pixel rows, see `Look::border_rows`.
    border_rows: u32,
    mica: [f32; 3],
    outline: Option<ID2D1PathGeometry>,
    noise: f32,
    dither_tile: Option<windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush>,
    shadow: Option<((i32, i32), windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush)>,
    px: f32,
    vignette: Option<((i32, i32, i32), windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush)>,
    free: Option<(f32, f32)>,
    width: f32,
    cx: Spring,
    w: Spring,
    d: Spring,
    pub open: Option<Panel>,
    open_part: Option<Part>,
    shown: Option<Panel>,
    anchor: (f32, f32),
    t: f32,
    phase_at: f32,
    pending: Option<(f32, Pending)>,

    mouse: Option<(f32, f32)>,
    candidate: Option<Part>,
    candidate_at: f32,
    hover: Option<Part>,
    hover_prev: Option<Part>,
    hover_at: f32,

    segments: Vec<Box<dyn Segment>>,
    hov: Vec<f32>,
    plate_hov: [f32; 2],
    player_panel: PlayerPanel,
    weather_panel: WeatherPanel,
    note_panel: NotePanel,
    stack_panel: StackPanel,
    launcher: LauncherPanel,
    text_running: bool,
    menu: MenuPanel,
    zone: Zone,
    dw: IDWriteFactory,
    app_icons: std::collections::HashMap<String, Option<windows::Win32::Graphics::Direct2D::ID2D1Bitmap1>>,
    popup_pending: Option<Part>,
    popup: Option<(Part, Option<std::time::Instant>)>,
    quiet: bool,
    playing: bool,
    tip: access::Tip,
    /// A text field took the mouse press: moves select in it until the release.
    pressing: Option<Panel>,

    texts: Texts,
    brushes: Option<Brushes>,
    parts: Vec<(Part, f32, f32)>,
    blocks: Vec<Rect>,
    theme: kit::Theme,
    search: Rect,
    scroller: Option<scroller::Scroller>,
    scroll: Option<scroller::Request>,
    hotspots: Vec<Hotspot>,
    /// Texts cut to fit in the last frame, see `Frame::cut`.
    cut: Vec<(Rect, String)>,
}

impl Island {
    pub fn palette(&self) -> kit::Palette {
        self.theme.palette
    }

    pub fn set_look(&mut self, light: bool, bg: crate::look::Rgb, look: &crate::look::Look, edge_x: f32) {
        self.theme.apply_look(light, bg, look.accent);
        self.looked = true;
        self.edge = Some((look.edge.clone(), edge_x));
        self.border = look.border.clone();
        self.border_rows = look.border_rows;
        self.noise = if light || look.noise < NOISE_MIN { 0.0 } else { look.noise };
        self.brushes = None;
        self.edge_brush = None;
    }

    pub fn plate_info(&self) -> String {
        let p = self.theme.plate;
        format!("tint {} × {:.2}", crate::look::hex([p.r, p.g, p.b]), p.a)
    }

    fn edge_tone(&self, l: f32, r: f32, px: f32) -> Option<([f32; 3], f32)> {
        let (cols, x0) = self.edge.as_ref()?;
        let i = |x: f32| (((x - x0) / px).max(0.0) as usize).min(cols.len().saturating_sub(1));
        let span = &cols[i(l)..=i(r).max(i(l))];
        let n = span.len() as f32;
        let c = span.iter().fold([0.0f32; 3], |a, c| [a[0] + c[0] / n, a[1] + c[1] / n, a[2] + c[2] / n]);
        let dl = (tone::lightness(c) - tone::lightness(self.mica)).abs();
        let t = ((dl - 0.05) / 0.30).clamp(0.0, 1.0);
        Some((c, t * t * (3.0 - 2.0 * t)))
    }

    pub fn bar_span(&self) -> Option<(f32, f32)> {
        let rects = self.blocks.iter().chain([&self.search]);
        let l = rects.clone().map(|b| b.0).fold(f32::MAX, f32::min);
        let r = rects.map(|b| b.2).fold(f32::MIN, f32::max);
        (r > l).then_some((l, r))
    }

    pub fn new(dw: &IDWriteFactory, h: f32, width: f32, theme: kit::Theme) -> Result<Self> {
        let c = width / 2.0;
        let segments: Vec<Box<dyn Segment>> =
            vec![Box::new(segments::Player::default()), Box::new(segments::Time::default()), Box::new(segments::Weather::default())];
        let hov = vec![0.0; segments.len()];
        Ok(Self {
            h,
            width,
            cx: Spring::new(c),
            w: Spring::new(200.0),
            d: Spring::new(0.0),
            open: None,
            open_part: None,
            shown: None,
            anchor: (c, 200.0),
            t: 0.0,
            phase_at: 0.0,
            pending: None,
            mouse: None,
            candidate: None,
            candidate_at: 0.0,
            hover: None,
            hover_prev: None,
            hover_at: 0.0,
            segments,
            hov,
            plate_hov: [0.0; 2],
            player_panel: PlayerPanel::default(),
            weather_panel: WeatherPanel,
            note_panel: NotePanel::default(),
            stack_panel: StackPanel::default(),
            launcher: LauncherPanel::default(),
            bottom: false,
            win_h: h,
            glass: None,
            looked: false,
            edge: None,
            edge_brush: None,
            border: Vec::new(),
            border_rows: 1,
            mica: [0.0; 3],
            outline: None,
            noise: 0.0,
            dither_tile: None,
            shadow: None,
            px: 1.0,
            vignette: None,
            free: None,
            text_running: false,
            menu: MenuPanel::default(),
            zone: Zone::default(),
            dw: dw.clone(),
            app_icons: std::collections::HashMap::new(),
            popup_pending: None,
            popup: None,
            quiet: false,
            playing: false,
            tip: access::Tip::default(),
            pressing: None,
            texts: Texts::new(dw, theme.font.as_deref())?,
            brushes: None,
            parts: Vec::new(),
            blocks: Vec::new(),
            theme,
            search: (c, INSET, c, h - INSET),
            scroller: None,
            scroll: None,
            hotspots: Vec::new(),
            cut: Vec::new(),
        })
    }

    fn panel_size(&self, panel: Panel) -> (f32, f32) {
        match panel {
            Panel::Player => self.player_panel.size(),
            Panel::Weather => self.weather_panel.size(),
            Panel::Note => self.note_panel.size(),
            Panel::Stack => self.stack_panel.size(),
            Panel::Launcher => self.launcher.size(),
            Panel::Menu => self.menu.size(),
        }
    }

    pub fn set_frame(&mut self, bottom: bool, win_h: f32) {
        self.bottom = bottom;
        self.win_h = win_h;
    }

    pub fn set_glass(&mut self, brush: windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush, mica: [f32; 3]) {
        self.glass = Some(brush);
        self.mica = mica;
    }

    pub fn set_free(&mut self, free: Option<(f32, f32)>) {
        self.free = free;
    }

    fn bar(&self) -> (f32, f32) {
        if self.bottom { (self.win_h - self.h, self.win_h) } else { (0.0, self.h) }
    }

    fn edge_y(&self) -> f32 {
        if self.bottom { self.win_h - self.h } else { self.bar().1 }
    }

    fn flow_geom(&self) -> (f32, f32, f32) {
        let px = self.px;
        let snap = |v: f32| (v / px).round() * px;
        let (l, r) = (snap(self.cx.x - self.w.x / 2.0), snap(self.cx.x + self.w.x / 2.0));
        ((l + r) / 2.0, r - l, snap(self.d.x.max(0.0)))
    }

    fn flow_bounds(&self) -> (f32, f32, f32, f32) {
        let (cx, w, d) = self.flow_geom();
        let b = shape::flow_bounds(cx, w, d, self.edge_y());
        if self.bottom { shape::mirror_bounds(b, self.edge_y()) } else { b }
    }

    pub fn wanted_depth(&self) -> f32 {
        let open = self.open.map_or(0.0, |p| self.panel_size(p).1);
        open.max(self.d.x)
    }

    pub fn prewarm_launcher(&mut self, dpi: f32) {
        self.launcher.prewarm(dpi);
    }

    pub fn launcher_open(&self) -> bool {
        self.open == Some(Panel::Launcher)
    }

    pub fn menu_open(&self) -> bool {
        self.open == Some(Panel::Menu)
    }

    pub fn menu_show(&mut self, module: String, screen: proto::Screen, at: Option<(f32, f32)>) {
        match at {
            Some(span) => self.menu_open_as(module, screen, span, false, false),
            None if self.menu_open() && self.menu.module == module => {
                self.menu.push(screen);
                self.fit(Panel::Menu);
            }
            None => {}
        }
    }

    /// Opens a dropdown under `span`. `page`: a page of search in place of the launcher, with
    /// "back" to it; `wide`: wider than a taskbar menu. Both change the size, so they are set
    /// before it is measured.
    pub fn menu_open_as(&mut self, module: String, screen: proto::Screen, (x, w): (f32, f32), page: bool, wide: bool) {
        self.menu.show(module, screen);
        self.menu.page = page;
        self.menu.wide = wide;
        self.open_at(Panel::Menu, None, x, w);
    }

    /// Shows a new first screen in the module's open dropdown, keeping it open and in place.
    pub fn menu_restart(&mut self, module: &str, screen: proto::Screen) {
        if self.menu_open() && self.menu.module == module {
            let (page, wide) = (self.menu.page, self.menu.wide);
            self.menu.show(module.to_string(), screen);
            (self.menu.page, self.menu.wide) = (page, wide);
            self.fit(Panel::Menu);
        }
    }

    /// Text in the launcher's search line.
    pub fn launcher_query(&self) -> String {
        self.launcher.model.line.text.trim().to_string()
    }

    pub fn menu_module(&self) -> Option<&str> {
        self.menu_open().then_some(self.menu.module.as_str())
    }

    pub fn menu_key(&mut self, vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY, ctrl: bool, shift: bool) -> MenuEffect {
        let e = self.menu.key(vk, ctrl, shift);
        self.fit(Panel::Menu);
        e
    }

    pub fn menu_replace(&mut self, module: &str, screen: proto::Screen) {
        if self.menu_open() && self.menu.module == module {
            self.menu.replace(screen);
            self.fit(Panel::Menu);
        }
    }

    pub fn menu_replace_root(&mut self, module: &str, screen: proto::Screen) {
        if self.menu_open() && self.menu.module == module {
            self.menu.replace_root(screen);
        }
    }

    pub fn menu_title(&self, module: &str) -> Option<&str> {
        (self.menu_open() && self.menu.module == module).then(|| self.menu.title()).flatten()
    }

    pub fn menu_editing(&self) -> bool {
        self.menu_open() && self.menu.editing()
    }

    pub fn menu_char(&mut self, c: char) -> MenuEffect {
        self.menu.char(c)
    }

    pub fn search_span(&self) -> (f32, f32) {
        (self.search.0, self.search.2 - self.search.0)
    }

    pub fn menu_click(&mut self, i: usize) -> MenuEffect {
        self.menu.activate(i)
    }

    pub fn menu_back(&mut self) -> MenuEffect {
        let e = self.menu.back();
        self.fit(Panel::Menu);
        e
    }

    fn fit(&mut self, panel: Panel) {
        if self.open != Some(panel) {
            return;
        }
        let d = self.panel_size(panel).1;
        match &mut self.pending {
            // Still opening: it will open to the new size.
            Some((_, Pending::Depth(p))) => *p = d,
            None if (self.d.target - d).abs() > 0.5 => self.d.go(d, OPEN_D),
            _ => {}
        }
    }

    pub fn open_launcher(&mut self) {
        self.launcher.model.reset();
        self.toggle(Panel::Launcher, Part::Search);
    }

    pub fn launcher_key(&mut self, vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY, ctrl: bool, shift: bool) -> crate::launcher::Effect {
        let e = self.launcher.model.key(vk, ctrl, shift);
        self.fit_launcher();
        e
    }

    pub fn launcher_caret(&self) -> Option<(f32, f32, f32)> {
        self.launcher_open().then_some(self.launcher.caret).flatten()
    }

    pub fn launcher_char(&mut self, c: char) -> crate::launcher::Effect {
        let e = self.launcher.model.char(c);
        self.fit_launcher();
        e
    }

    pub fn launcher_click(&mut self, i: usize) -> crate::launcher::Effect {
        self.launcher.model.activate(i)
    }

    pub fn pick_swatch(&mut self, i: usize) -> Option<String> {
        self.note_panel.pick(i)
    }

    pub fn launcher_wheel(&mut self, notches: f32) {
        if self.launcher_open() {
            self.launcher.wheel(notches);
        }
    }

    pub fn launcher_act(&mut self, j: usize) -> crate::launcher::Effect {
        let e = self.launcher.model.act(j);
        self.fit_launcher();
        e
    }

    pub fn launcher_menu_at(&mut self, x: f32, y: f32) -> Option<crate::launcher::Effect> {
        let HotKind::Launch(i) = self.hotspot_at(x, y)?.kind else { return None };
        let e = if self.launcher.model.menu.as_ref().is_some_and(|m| m.row == i) {
            self.launcher.model.menu = None;
            crate::launcher::Effect::Redraw
        } else {
            self.launcher.model.open_menu(i)
        };
        self.fit_launcher();
        Some(e)
    }

    pub fn sync_launcher(&mut self) -> bool {
        if self.launcher_open() && self.launcher.model.sync() {
            self.fit_launcher();
            return true;
        }
        false
    }

    fn fit_launcher(&mut self) {
        self.fit(Panel::Launcher);
    }

    fn sync_stack(&mut self) {
        if self.open != Some(Panel::Stack) {
            return;
        }
        let notes = self.zone.hidden_notes(STACK_ROWS);
        if notes.is_empty() {
            return self.close();
        }
        let resized = notes.len() != self.stack_panel.rows();
        self.stack_panel.set(notes);
        if resized {
            self.d.go(self.stack_panel.size().1, OPEN_D);
        }
    }

    pub fn notify(&mut self, note: Note) {
        let part = self.zone.notify(note);
        if self.open == Some(Panel::Note) && self.open_part == Some(part) {
            self.fill_note(part);
            self.d.go(self.note_panel.size().1, OPEN_D);
        }
        self.queue_popups();
    }

    pub fn expire_notes(&mut self) -> bool {
        let now = std::time::Instant::now();
        let mut changed = self.zone.expire(now);
        self.queue_popups();
        if let Some((part, Some(at))) = self.popup
            && at <= now
        {
            self.popup = None;
            if self.open == Some(Panel::Note) && self.open_part == Some(part) {
                self.close();
                changed = true;
            }
        }
        changed | self.popup_pending.is_some()
    }

    pub fn next_note_deadline(&self) -> Option<std::time::Instant> {
        let popup = self.popup.and_then(|(_, at)| at);
        self.zone.next_deadline().into_iter().chain(popup).min()
    }

    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    fn queue_popups(&mut self) {
        if let Some(p) = self.zone.take_popups().pop() {
            self.popup_pending = Some(p);
        }
    }

    fn show_popup(&mut self) {
        let Some(part) = self.popup_pending else { return };
        let ours = self.open == Some(Panel::Note) && self.popup.is_some_and(|(p, _)| Some(p) == self.open_part);
        if self.quiet || (self.open.is_some() && !ours) || self.zone.note(part).is_none() {
            self.popup_pending = None;
            return;
        }
        let Some((c, w)) = self.zone.anchor(part) else { return };
        self.popup_pending = None;
        self.fill_note(part);
        self.open_at(Panel::Note, Some(part), c - w / 2.0, w);
        self.popup = Some((part, Some(std::time::Instant::now() + POPUP_TTL)));
    }

    fn hold_popup(&mut self) {
        let Some((part, at)) = self.popup else { return };
        if self.open != Some(Panel::Note) || self.open_part != Some(part) {
            self.popup = None;
            return;
        }
        let over = self.mouse.is_some_and(|(x, y)| matches!(self.hit(x, y), Hit::Panel) || self.zone.hit(x, y) == Some(part));
        self.popup = Some((
            part,
            match (over, at) {
                (true, _) => None,
                (false, None) => Some(std::time::Instant::now() + POPUP_TTL),
                (false, at) => at,
            },
        ));
    }

    pub fn note_app(&self, part: Part) -> Option<String> {
        self.zone.note(part)?.app.clone()
    }

    pub fn dismiss_note(&mut self, part: Part) {
        self.zone.dismiss(part);
    }

    pub fn dismiss_key(&mut self, key: &str) -> bool {
        let part = self.zone.dismiss_key(key);
        part.is_some() && self.open == Some(Panel::Note) && self.open_part == part
    }

    pub fn take_zone(&mut self) -> Zone {
        std::mem::take(&mut self.zone)
    }

    pub fn set_zone(&mut self, zone: Zone) {
        self.zone = zone;
    }

    pub fn next_wake(&self) -> Option<f32> {
        let glyph = self.zone.shows(logo::Anim::Glyph).then(logo::next_glyph);
        // A field being typed in blinks its caret without continuous frames.
        let blink = match self.open {
            Some(Panel::Launcher) => self.launcher.field.next_blink(),
            Some(Panel::Menu) => self.menu.next_blink(),
            _ => None,
        };
        [glyph, blink].into_iter().flatten().reduce(f32::min)
    }

    /// The mouse went down: a text field puts its caret there. `true` if a field took the press;
    /// the release is then not a click.
    pub fn press(&mut self, x: f32, y: f32, shift: bool) -> bool {
        if self.launcher_open() && self.launcher.field.contains(x, y) {
            self.launcher.field.press(x, shift);
            self.pressing = Some(Panel::Launcher);
            return true;
        }
        if self.menu_open()
            && let Some(HotKind::MenuRow(i)) = self.hotspot_at(x, y).map(|h| h.kind)
            && self.menu.press(i, x, shift)
        {
            self.pressing = Some(Panel::Menu);
            return true;
        }
        false
    }

    /// The mouse moved with the button down: the field's selection follows. `true` if it did.
    pub fn drag(&mut self, x: f32) -> bool {
        match self.pressing {
            Some(Panel::Launcher) => self.launcher.field.drag(x),
            Some(Panel::Menu) => self.menu.drag(x),
            _ => return false,
        }
        true
    }

    /// The button went up; `true` if the press belonged to a field.
    pub fn release(&mut self) -> bool {
        self.pressing.take().is_some()
    }

    /// Whether the cursor is over a text field: it shows the text cursor there.
    pub fn over_field(&self, x: f32, y: f32) -> bool {
        (self.launcher_open() && self.launcher.field.contains(x, y))
            || (self.menu_open() && matches!(self.hotspot_at(x, y).map(|h| h.kind), Some(HotKind::MenuRow(i)) if self.menu.is_input(i)))
    }

    pub fn dismiss_open(&mut self) {
        if let Some(part) = self.open_part {
            self.zone.dismiss(part);
        }
    }

    fn fill_note(&mut self, part: Part) {
        if let Some(n) = self.zone.note(part)
            && let Err(e) = self.note_panel.set(&self.texts, &self.dw, n)
        {
            proto::error!("ui", "note panel: {e}");
        }
    }

    pub fn step(&mut self, dt: f32) -> bool {
        self.t += dt;
        if let Some((at, action)) = self.pending
            && self.t >= at
        {
            self.pending = None;
            match action {
                Pending::Depth(d) => self.d.go(d, OPEN_D),
                Pending::Retract { cx, w } => {
                    self.cx.go(cx, CLOSE);
                    self.w.go(w, CLOSE);
                    self.d.go(0.0, CLOSE);
                }
            }
        }
        let a = self.cx.step(dt);
        let b = self.w.step(dt);
        let c = self.d.step(dt);
        if self.open.is_none() && self.d.x <= 0.5 && self.pending.is_none() {
            self.shown = None;
        }

        let hover_waiting = self.candidate != self.hover;
        if hover_waiting && (self.candidate.is_none() || self.t - self.candidate_at >= HOVER_DELAY) {
            self.hover = self.candidate;
        }
        if self.hover != self.hover_prev {
            self.hover_prev = self.hover;
            self.hover_at = self.t;
        }
        let mut moving = false;
        for (seg, v) in self.segments.iter_mut().zip(self.hov.iter_mut()) {
            let own_panel_open = seg.panel().is_some() && seg.panel() == self.open;
            let target = if self.hover == Some(seg.part()) && seg.hoverable() && !own_panel_open { 1.0 } else { 0.0 };
            moving |= tween(v, target, dt / HOVER_FADE);
            moving |= seg.step(dt);
        }
        let dtw = self.hover.is_some_and(|h| h != Part::Player && self.parts.iter().any(|(p, ..)| *p == h));
        let search = self.hover == Some(Part::Search);
        moving |= tween(&mut self.plate_hov[0], if dtw { 1.0 } else { 0.0 }, dt / HOVER_FADE);
        moving |= tween(&mut self.plate_hov[1], if search { 1.0 } else { 0.0 }, dt / HOVER_FADE);
        let open_part = self.open.and(self.open_part);
        moving |= self.zone.step(dt, self.hover, self.candidate, open_part, HOVER_FADE);
        moving |= self.step_tip(dt);
        self.queue_popups();
        self.show_popup();
        self.hold_popup();
        moving |= self.popup_pending.is_some();
        self.sync_stack();
        moving |= self.sync_launcher();

        let content = self.shown.is_some() && self.t - self.phase_at < CONTENT_START + CONTENT_DUR + CONTENT_STAGGER * STAGGER_ROWS as f32;
        a | b | c | content | self.pending.is_some() | hover_waiting | moving
    }

    pub fn needs_ticks(&self) -> bool {
        let segments = self.segments.iter().zip(&self.hov).any(|(s, h)| s.needs_ticks(*h));
        segments || (self.playing && self.shown == Some(Panel::Player))
    }

    pub fn wants_frames(&self) -> bool {
        let logo = self.zone.shows(logo::Anim::Dots);
        let launcher = (self.launcher_open() && self.launcher.moving())
            || (self.menu_open() && self.menu.moving())
            || (self.open == Some(Panel::Stack) && self.stack_panel.moving());
        self.segments.iter().zip(&self.hov).any(|(s, h)| s.wants_frames(*h)) || self.zone.wants_frames() || logo || launcher || self.text_running
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let prev = self.mouse.and_then(|(x, y)| self.hotspot_at(x, y)).map(|h| h.rect);
        self.mouse = Some((x, y));
        let part = match self.hit(x, y) {
            Hit::Part(p) => Some(p),
            _ => None,
        };
        let mut redraw = false;
        if part != self.candidate {
            self.candidate = part;
            self.candidate_at = self.t;
            redraw = true;
        }
        redraw | (self.hotspot_at(x, y).map(|h| h.rect) != prev)
    }

    pub fn mouse_leave(&mut self) {
        self.mouse = None;
        self.candidate = None;
    }

    pub fn toggle(&mut self, panel: Panel, anchor: Part) {
        self.popup = None;
        if self.open == Some(panel) && self.open_part == Some(anchor) {
            return self.close();
        }
        let found = self.parts.iter().find(|(p, ..)| *p == anchor).map(|&(_, x, w)| (x, w));
        let search = (anchor == Part::Search).then_some((self.search.0, self.search.2 - self.search.0));
        let Some((x, w)) = found.or(search).or_else(|| self.zone.anchor(anchor).map(|(c, w)| (c - w / 2.0, w))) else { return };
        match panel {
            Panel::Note => self.fill_note(anchor),
            Panel::Stack => self.stack_panel.set(self.zone.hidden_notes(STACK_ROWS)),
            _ => {}
        }
        self.open_at(panel, Some(anchor), x, w);
    }

    fn open_at(&mut self, panel: Panel, part: Option<Part>, x: f32, w: f32) {
        let (pw, pd) = self.panel_size(panel);
        let cx = (x + w / 2.0).clamp(pw / 2.0 + 20.0, self.width - pw / 2.0 - 20.0);
        self.anchor = (x + w / 2.0, w);
        if self.d.x < 1.0 {
            self.cx.snap(x + w / 2.0);
            self.w.snap(w);
            self.pending = Some((self.t + OPEN_D_DELAY, Pending::Depth(pd)));
        } else {
            self.pending = None;
            self.d.go(pd, OPEN_D);
        }
        self.cx.go(cx, OPEN_W);
        self.w.go(pw, OPEN_W);
        self.open = Some(panel);
        self.open_part = part;
        self.shown = Some(panel);
        self.phase_at = self.t;
    }

    pub fn close_now(&mut self) {
        self.open = None;
        self.shown = None;
        self.pending = None;
        let (cx, w) = self.anchor;
        self.cx.snap(cx);
        self.w.snap(w);
        self.d.snap(0.0);
    }

    pub fn close(&mut self) {
        if self.open.is_none() {
            return;
        }
        self.open = None;
        self.phase_at = self.t;
        let (cx, w) = self.anchor;
        self.pending = Some((self.t + CLOSE_DELAY, Pending::Retract { cx, w }));
    }

    fn hotspot_at(&self, x: f32, y: f32) -> Option<&Hotspot> {
        self.hotspots.iter().find(|h| inside(h.rect, x, y))
    }

    pub fn click(&self, x: f32, y: f32) -> Action {
        if let Some(h) = self.hotspot_at(x, y) {
            return match h.kind {
                HotKind::Media(cmd) => Action::Media(cmd),
                HotKind::Dismiss => Action::Dismiss,
                HotKind::Note(p) => Action::Open(Panel::Note, p),
                HotKind::Launch(i) => Action::Launch(i),
                HotKind::LaunchAct(j) => Action::LaunchAct(j),
                HotKind::Swatch(i) => Action::Swatch(i),
                HotKind::OpenApp => self.open_part.map_or(Action::None, Action::OpenApp),
                HotKind::MenuRow(i) => Action::MenuRow(i),
                HotKind::MenuBack => Action::MenuBack,
                HotKind::Seek { duration } => {
                    let (l, _, r, _) = h.rect;
                    let frac = ((x - l) / (r - l)).clamp(0.0, 1.0) as f64;
                    Action::Media(Cmd::Seek(frac * duration))
                }
            };
        }
        match self.hit(x, y) {
            Hit::Part(p @ Part::Note(_)) if self.zone.note(p).is_some_and(|n| n.app.is_some()) => Action::OpenApp(p),
            Hit::Part(p @ Part::Note(_)) => Action::Open(Panel::Note, p),
            Hit::Part(Part::More) => Action::Open(Panel::Stack, Part::More),
            Hit::Part(Part::Search) => Action::Open(Panel::Launcher, Part::Search),
            Hit::Part(p) => self.segments.iter().find(|s| s.part() == p).map_or(Action::None, |s| s.click()),
            Hit::None if self.d.x > 0.5 => Action::Close,
            _ => Action::None,
        }
    }

    pub fn hit(&self, x: f32, y: f32) -> Hit {
        if self.blocks.iter().any(|b| inside(*b, x, y)) {
            return self.parts.iter().find(|(_, px, pw)| x >= px - BLOCK_INNER && x <= px + pw + BLOCK_INNER).map_or(Hit::Pill, |(p, ..)| Hit::Part(*p));
        }
        if inside(self.search, x, y) {
            return Hit::Part(Part::Search);
        }
        if let Some(p) = self.zone.hit(x, y) {
            return Hit::Part(p);
        }
        if self.d.x > 0.5 && inside(self.flow_bounds(), x, y) {
            let on = match &self.outline {
                // SAFETY: Direct2D call on a live geometry with plain value arguments.
                Some(geo) => unsafe { geo.FillContainsPoint(Vector2 { X: x, Y: y }, None, 0.25) }.map(|b| b.as_bool()).unwrap_or(true),
                None => true,
            };
            if on {
                return Hit::Panel;
            }
        }
        Hit::None
    }

    pub fn regions(&self) -> Vec<Rect> {
        let mut v = self.blocks.clone();
        v.push(self.search);
        v.extend(self.tip.drawn);
        v.extend(self.zone.rects());
        if self.d.x > 0.5 {
            let w = self.open.map_or(self.w.x, |p| self.panel_size(p).0.max(self.w.x));
            let b = shape::flow_bounds(self.cx.x, w, self.wanted_depth(), self.edge_y());
            let grown = if self.bottom { shape::mirror_bounds(b, self.edge_y()) } else { b };
            v.push(grown);
            if self.looked {
                let (l, t, r, b) = grown;
                v.push(if self.bottom { (l - SHADOW_PAD, t - SHADOW_PAD, r + SHADOW_PAD, b) } else { (l - SHADOW_PAD, t, r + SHADOW_PAD, b + SHADOW_PAD) });
            }
            if self.looked {
                let (l, _, r, _) = grown;
                let y = self.edge_y();
                // the outline can be two rows thick (see `Look::border_rows`)
                let t = self.border_rows.max(1) as f32;
                v.push((l - BORDER_TAIL - 1.0, y - t, r + BORDER_TAIL + 1.0, y + t));
            }
        }
        v
    }

    pub fn draw(&mut self, gfx: &Gfx, dc: &ID2D1DeviceContext, m: &media::State, wx: Option<&weather::Weather>) -> Result<()> {
        let res = self.draw_frame(gfx, dc, &Data { media: m, weather: wx });
        self.texts.end_frame();
        res
    }

    pub fn draw_layers(&mut self, gfx: &Gfx) -> Result<()> {
        match self.scroll.take() {
            Some(req) => {
                if self.scroller.is_none() {
                    self.scroller = Some(scroller::Scroller::new(gfx)?);
                }
                let fg = self.brushes.as_ref().map(|b| b.fg.clone());
                if let (Some(s), Some(fg)) = (self.scroller.as_mut(), fg) {
                    s.show(gfx, &req, &fg)?;
                }
            }
            None => {
                if let Some(s) = self.scroller.as_mut() {
                    s.hide()?;
                }
            }
        }
        Ok(())
    }

    fn draw_frame(&mut self, gfx: &Gfx, dc: &ID2D1DeviceContext, data: &Data) -> Result<()> {
        if self.brushes.is_none() {
            self.brushes = Some(Brushes::new(dc, &self.theme)?);
        }
        if let Some(br) = &self.brushes {
            // The average taskbar edge, as the dropdown outline takes it; before the first capture, a
            // faint text color.
            let n = self.border.len() as f32;
            let c = if n > 0.0 {
                let sum = self.border.iter().fold([0.0f32; 3], |a, c| [a[0] + c[0], a[1] + c[1], a[2] + c[2]]);
                windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: sum[0] / n, g: sum[1] / n, b: sum[2] / n, a: 1.0 }
            } else {
                windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { a: 0.18, ..self.theme.palette.fg }
            };
            // SAFETY: Direct2D call on a live brush with a value argument.
            unsafe { br.edge.SetColor(&c) };
        }
        if let Some(p) = &self.texts.hinted {
            // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
            // outlives the call.
            unsafe { dc.SetTextRenderingParams(p) };
        }
        self.hotspots.clear();
        self.cut.clear();
        self.playing = data.media.playing;
        let reveal = Reveal { since: self.t - self.phase_at, opening: self.open.is_some() };
        self.px = 96.0 / gfx.dpi;
        let (cx, w, d) = self.flow_geom();
        let edge = self.edge_y();
        let outline = match shape::flow(cx, w, d, edge, if self.looked { 0.0 } else { 1.0 }).map(|o| if self.bottom { o.mirror(edge) } else { o }) {
            Some(o) => Some(outline_geometry(gfx, &o, true)?),
            None => None,
        };
        self.outline = outline.clone();

        let (bar_t, bar_b) = self.bar();
        let body = self.theme.palette.body;
        let backdrop = [body.r, body.g, body.b];
        if self.edge_brush.is_none() && self.looked {
            self.edge_brush = match &self.edge {
                Some((cols, x)) => edge_brush(gfx, dc, cols, *x).ok(),
                None => None,
            };
        }
        let fb = self.flow_bounds();
        let (fl, fr) = (fb.0, fb.2);
        let tone = if self.looked { self.edge_tone(fl, fr, 96.0 / gfx.dpi) } else { None };
        let full = self.shown.map_or((w, d), |p| self.panel_size(p));
        let rim = tone.map(|(_, k)| VIGNETTE.0 + (VIGNETTE.1 - VIGNETTE.0) * k);
        if let Some(rim) = rim {
            let key = (full.0.round() as i32, full.1.round() as i32, (rim * 100.0).round() as i32);
            if self.vignette.as_ref().is_none_or(|(k, _)| *k != key) {
                self.vignette = bake_vignette(gfx, dc, full, rim).ok().map(|b| (key, b));
            }
        }
        let edge_fade = match (&self.edge_brush, self.d.x > 0.5, rim) {
            (Some(brush), true, Some(rim)) => {
                let (l, t, r, b) = self.flow_bounds();
                let dir = if self.bottom { -1.0 } else { 1.0 };
                let seam = kit::linear_fade(dc, edge, edge + dir * (full.1 * SEAM_SHARE).max(EDGE_FADE));
                let top = if self.bottom { edge - d } else { edge };
                let vignette: Option<windows::Win32::Graphics::Direct2D::ID2D1Brush> = match &self.vignette {
                    Some((_, v)) => {
                        // SAFETY: DirectComposition call on a live visual; the matrix outlives the call.
                        unsafe { v.SetTransform(&Matrix3x2 { M11: w / full.0, M12: 0.0, M21: 0.0, M22: d / full.1, M31: cx - w / 2.0, M32: top }) };
                        v.cast().ok()
                    }
                    None => kit::vignette(dc, cx, top + d / 2.0, w * VIGNETTE_SIZE, d * VIGNETTE_SIZE, rim).and_then(|v| v.cast()).ok(),
                };
                match (seam, vignette) {
                    (Ok(seam), Some(vignette)) => {
                        Some(kit::EdgeFade { brush: brush.clone(), seam, vignette, rect: D2D_RECT_F { left: l, top: t, right: r, bottom: b } })
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        let dither = if self.noise > 0.0 && self.looked {
            if self.dither_tile.is_none() {
                self.dither_tile = dither_tile(gfx, dc).ok();
            }
            self.dither_tile.clone().map(|t| (t, (self.noise / TILE_DIFF).min(1.0)))
        } else {
            None
        };
        let shadow_full = full;
        if self.looked && self.d.x > 0.5 {
            let key = (shadow_full.0.round() as i32, shadow_full.1.round() as i32);
            if self.shadow.as_ref().is_none_or(|(k, _)| *k != key) {
                self.shadow = bake_shadow(gfx, dc, shadow_full).ok().map(|b| (key, b));
            }
        }
        let mut f = Frame {
            dither,
            edge: edge_fade,
            backdrop,
            glass: self.looked,
            gfx,
            dc,
            br: self.brushes.as_ref().unwrap(),
            texts: &mut self.texts,
            hotspots: &mut self.hotspots,
            cut: &mut self.cut,
            mouse: self.mouse,
            t: self.t,
            scroll: &mut self.scroll,
            app_icons: &mut self.app_icons,
            running: std::cell::Cell::new(false),
        };

        // ── Layout ──
        // Segments are grouped into blocks: player; time and weather; then search and the right zone.
        // The anchor block (time) is centered by its resting width, so a segment that widens
        // on hover grows to the right and does not shift the time.
        let mut rest = Vec::with_capacity(self.segments.len());
        let mut widths = Vec::with_capacity(self.segments.len());
        for (s, h) in self.segments.iter_mut().zip(&self.hov) {
            let r = s.measure(&mut f, data)?;
            rest.push(r);
            widths.push(r + (s.hover_width(r) - r) * ease_in_out(*h));
        }
        let block_of = |p: Part| usize::from(p != Part::Player);
        let parts: Vec<Part> = self.segments.iter().map(|s| s.part()).collect();
        let anchor = block_of(ANCHOR);
        let block_w = |b: usize, ws: &[f32]| {
            let inner: Vec<f32> = parts.iter().zip(ws).filter(|(p, _)| block_of(**p) == b).map(|(_, w)| *w).collect();
            BLOCK_PAD * 2.0 + inner.iter().sum::<f32>() + (inner.len().saturating_sub(1)) as f32 * (BLOCK_INNER * 2.0 + DIVIDER_W)
        };
        let (top, bottom) = (bar_t + INSET, bar_b - INSET);
        let mut lefts = [0.0f32; 2];
        match self.free {
            Some((fl, _)) => {
                lefts[anchor] = (fl + FREE_PAD).round();
                let search_l = lefts[anchor] + block_w(anchor, &widths) + BLOCK_GAP;
                lefts[0] = search_l + (bottom - top) + BLOCK_GAP;
            }
            None => {
                let anchor_l = (self.width / 2.0 - block_w(anchor, &rest) / 2.0).round();
                lefts[anchor] = anchor_l;
                lefts[0] = anchor_l - BLOCK_GAP - block_w(0, &widths);
            }
        }
        self.blocks = (0..2).map(|b| (lefts[b], top, lefts[b] + block_w(b, &widths), bottom)).collect();
        self.parts.clear();
        let mut dividers = Vec::new();
        for (b, left) in lefts.iter().enumerate() {
            let mut x = left + BLOCK_PAD;
            let mut first = true;
            for (p, w) in parts.iter().zip(&widths).filter(|(p, _)| block_of(**p) == b) {
                if !first {
                    dividers.push(x + BLOCK_INNER);
                    x += BLOCK_INNER * 2.0 + DIVIDER_W;
                }
                first = false;
                self.parts.push((*p, x, *w));
                x += w;
            }
        }
        self.parts.sort_by_key(|(p, ..)| parts.iter().position(|q| q == p));
        let (t, b) = (top, bottom);
        let cy = (t + b) / 2.0;
        let radius = (b - t) / 2.0;

        // ── Shape ──
        let mut flat_fill: Option<(
            windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
            f32,
            Option<windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush>,
        )> = None;
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe {
            let base = self.theme.palette.glass_tint;
            let tinted = match tone {
                Some((c, k)) => {
                    let body = self.theme.palette.body;
                    let mix = |a: f32, b: f32| a + (b - a) * k;
                    let color =
                        windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: mix(body.r, c[0]), g: mix(body.g, c[1]), b: mix(body.b, c[2]), a: 1.0 };
                    Some((dc.CreateSolidColorBrush(&color, None)?, base + (TINT_MAX - base) * k))
                }
                None => None,
            };
            let (body_brush, tint) = match &tinted {
                Some((b, t)) => (b, *t),
                None => (&f.br.body, base),
            };
            let flat = self.looked && self.shown.is_some();
            if flat {
                flat_fill = Some((body_brush.clone(), tint, self.glass.clone()));
            }
            if let (false, Some(o)) = (flat, &outline) {
                fill_flow(dc, body_brush, tint, o, self.glass.as_ref());
            }
            let (l, _, r, _) = self.blocks[anchor];
            draw_plate(dc, &f.br.surface, (l, t, r, b), self.plate_hov[0]);
            f.br.dim.SetOpacity(DIVIDER_ALPHA);
            for x in &dividers {
                dc.FillRectangle(&D2D_RECT_F { left: *x, top: cy - DIVIDER_H / 2.0, right: x + DIVIDER_W, bottom: cy + DIVIDER_H / 2.0 }, &f.br.dim);
            }
            f.br.dim.SetOpacity(1.0);
        }

        // ── Segments ──
        for ((s, &(_, x, w)), h) in self.segments.iter_mut().zip(&self.parts).zip(&self.hov) {
            s.draw(&mut f, data, x, w, cy, Hover { p: *h, since: self.hover_at })?;
        }

        // ── Search Block ──
        let sl = self.blocks[anchor].2 + BLOCK_GAP;
        let size = b - t;
        self.search = (sl, t, sl + size, b);
        draw_plate(dc, &f.br.surface, (sl, t, sl + size, b), self.plate_hov[1]);
        crate::icons::draw(gfx, dc, crate::icons::Icon::Search, sl + radius, cy, SEARCH_ICON, &f.br.fg)?;

        // ── Right Zone ──
        // A block without a plate. There is no room past the window edge: extra chips fold into "+N".
        let zone_r = self.free.map_or(self.width / 2.0 + ISLAND_W / 2.0, |(_, fr)| fr) - ZONE_EDGE;
        let zone_l = if self.free.is_some() { self.blocks[0].2 + BLOCK_GAP } else { sl + size + BLOCK_GAP + BLOCK_PAD };
        self.zone.draw(&mut f, zone_l, zone_r, t, b, self.hover_at)?;

        // ── Panel Content ──
        if let (Some(panel), Some(geo)) = (self.shown, &outline) {
            let mask: ID2D1Geometry = geo.cast()?;
            let params = D2D1_LAYER_PARAMETERS1 {
                contentBounds: D2D_RECT_F { left: -f32::MAX, top: -f32::MAX, right: f32::MAX, bottom: f32::MAX },
                geometricMask: std::mem::ManuallyDrop::new(Some(mask)),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS1_NONE,
            };
            let area = Area { cx, w, d, top: if self.bottom { edge - d } else { edge } };
            if let (true, Some((_, shadow))) = (flat_fill.is_some(), &self.shadow) {
                draw_shadow(dc, shadow, (cx, w, d), full, edge, self.bottom, self.width);
            }
            // SAFETY: Direct2D calls on live COM objects; `params` and its mask outlive the calls.
            unsafe { dc.PushLayer(&params, None) };
            if let Some((body, tint, glass)) = &flat_fill {
                let rect = D2D_RECT_F { left: fb.0, top: fb.1, right: fb.2, bottom: fb.3 };
                // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
                // outlives the call.
                unsafe {
                    match glass {
                        Some(g) => {
                            dc.FillRectangle(&rect, g);
                            body.SetOpacity(*tint);
                        }
                        None => body.SetOpacity(1.0),
                    }
                    dc.FillRectangle(&rect, body);
                    body.SetOpacity(1.0);
                }
            }
            if panel != Panel::Player {
                f.draw_edge(true);
            }
            let res = match panel {
                Panel::Player => self.player_panel.draw(&mut f, data, area, reveal),
                Panel::Weather => self.weather_panel.draw(&mut f, data, area, reveal),
                Panel::Note => self.note_panel.draw(&mut f, data, area, reveal),
                Panel::Stack => self.stack_panel.draw(&mut f, data, area, reveal),
                Panel::Launcher => self.launcher.draw(&mut f, data, area, reveal),
                Panel::Menu => self.menu.draw(&mut f, data, area, reveal),
            };
            // SAFETY: Direct2D call on a live context; pops the layer pushed above.
            unsafe { dc.PopLayer() };
            drop(std::mem::ManuallyDrop::into_inner(params.geometricMask));
            res?;
        }
        access::draw_tip(&mut self.tip, &mut f, self.width, self.win_h, self.bottom)?;
        self.text_running = f.running.get();
        // ── Border ──
        // The taskbar edge is erased under the dropdown and continued along its outline.
        if let (true, Some(brush), Some((_, edge_x)), Some(o), Some((panel, _))) =
            (self.looked && !self.border.is_empty(), &self.edge_brush, &self.edge, shape::flow(cx, w, d, edge, 0.0), tone)
        {
            let (l, _, r, _) = self.flow_bounds();
            let o = if self.bottom { o.mirror(edge) } else { o };
            draw_border(gfx, dc, brush, &self.border, self.border_rows, panel, *edge_x, &o, self.bottom, edge, (l, r))?;
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_border(
    gfx: &Gfx,
    dc: &ID2D1DeviceContext,
    edge_brush: &windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush,
    border: &[crate::look::Rgb],
    rows: u32,
    panel: crate::look::Rgb,
    edge_x: f32,
    o: &shape::Outline,
    bottom: bool,
    edge: f32,
    (l, r): (f32, f32),
) -> Result<()> {
    let px = 96.0 / gfx.dpi;
    let col = |x: f32| (((x - edge_x) / px).max(0.0) as usize).min(border.len().saturating_sub(1));
    let span = &border[col(l)..=col(r).max(col(l))];
    let n = span.len() as f32;
    let c = span.iter().fold([0.0f32; 3], |a, c| [a[0] + c[0] / n, a[1] + c[1] / n, a[2] + c[2] / n]);
    if (tone::lightness(c) - tone::lightness(panel)).abs() < BORDER_MIN_DL {
        return Ok(());
    }
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        // the native outline is `rows` pixels thick: erase and continue all of it
        let t = px * rows.max(1) as f32;
        let row = if bottom { (edge, edge + t) } else { (edge - t, edge) };
        let (sl, sr) = (((l - BORDER_TAIL) / px).floor() * px, ((r + BORDER_TAIL) / px).ceil() * px);
        dc.FillRectangle(&D2D_RECT_F { left: sl, top: row.0, right: sr, bottom: row.1 }, edge_brush);
        let mut segs = vec![shape::Seg::Line(o.start.0, o.start.1)];
        segs.extend(o.segs.iter().copied());
        segs.push(shape::Seg::Line(sr, edge));
        let tailed = shape::Outline { start: (sl, edge), segs };
        let geo = outline_geometry(gfx, &tailed, false)?;
        let shift = if bottom { t / 2.0 } else { -t / 2.0 };
        let moved = gfx.d2d.CreateTransformedGeometry(&geo, &Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: shift })?;
        let brush = dc.CreateSolidColorBrush(&windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a: 1.0 }, None)?;
        dc.DrawGeometry(&moved, &brush, t, None);
    }
    Ok(())
}

fn outline_geometry(gfx: &Gfx, o: &shape::Outline, closed: bool) -> Result<ID2D1PathGeometry> {
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        let geo = gfx.d2d.CreatePathGeometry()?;
        let sink = geo.Open()?;
        sink.BeginFigure(Vector2 { X: o.start.0, Y: o.start.1 }, D2D1_FIGURE_BEGIN_FILLED);
        for s in &o.segs {
            match *s {
                shape::Seg::Line(x, y) => sink.AddLine(Vector2 { X: x, Y: y }),
                shape::Seg::Arc { x, y, r, clockwise } => sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: Vector2 { X: x, Y: y },
                    size: D2D_SIZE_F { width: r, height: r },
                    rotationAngle: 0.0,
                    sweepDirection: if clockwise { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
                    arcSize: D2D1_ARC_SIZE_SMALL,
                }),
            }
        }
        sink.EndFigure(if closed { D2D1_FIGURE_END_CLOSED } else { windows::Win32::Graphics::Direct2D::Common::D2D1_FIGURE_END_OPEN });
        sink.Close()?;
        Ok(geo.into())
    }
}

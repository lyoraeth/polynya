// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared rendering utilities for segments and panels: fonts, text cache, brushes,
//! marquees, buttons, progress bars, and clickable zones.
//!
//! Components receive all of this via `Frame`, the context for a single frame.

use std::collections::HashMap;

use windows::{
    Win32::Graphics::{
        Direct2D::{
            Common::{D2D_RECT_F, D2D1_COLOR_F, D2D1_GRADIENT_STOP},
            D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED, D2D1_COLOR_SPACE_SRGB,
            D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
            D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT, ID2D1DeviceContext, ID2D1GradientStopCollection1, ID2D1LinearGradientBrush,
            ID2D1SolidColorBrush,
        },
        DirectWrite::{
            DWRITE_FONT_FEATURE, DWRITE_FONT_FEATURE_TAG_CASE_SENSITIVE_FORMS, DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, DWRITE_FONT_STRETCH_NORMAL,
            DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
            IDWriteFactory, IDWriteFactory5, IDWriteFontCollection, IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1,
        },
    },
    core::{BOOL, HSTRING, IUnknown, Interface, PCWSTR, Result, w},
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::{
    gfx::Gfx,
    icons::{self, Icon},
    media::{self, Cmd},
};

// ── Palettes ──

/// Theme colors: text and island body blend with the taskbar.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub fg: D2D1_COLOR_F,
    pub dim: D2D1_COLOR_F,
    /// Secondary text is closer to primary than dimmed (e.g., notification body).
    pub soft: D2D1_COLOR_F,
    pub track: D2D1_COLOR_F,
    /// Island body: flow and capsule.
    pub body: D2D1_COLOR_F,
    /// Opacity of the island body masking blurred wallpaper in glass mode.
    pub glass_tint: f32,
}

pub const DARK: Palette = Palette {
    fg: rgb(0xec, 0xec, 0xec),
    // Maintains APCA Lc 45 contrast on dark taskbar and lighter plates.
    dim: rgb(0xa8, 0xa8, 0xa8),
    soft: rgb(0xca, 0xca, 0xca),
    track: rgb(0x2a, 0x2a, 0x2a),
    body: rgb(0, 0, 0),
    glass_tint: 0.5,
};

/// Light theme: body matches Windows 11 light taskbar. White glass requires higher
/// opacity than dark, otherwise dark wallpapers bleed through and reduce text contrast.
pub const LIGHT: Palette = Palette {
    fg: rgb(0x1b, 0x1b, 0x1f),
    dim: rgb(0x55, 0x55, 0x5b),
    soft: rgb(0x3a, 0x3a, 0x40),
    track: rgb(0xd9, 0xd9, 0xdc),
    body: rgb(0xf3, 0xf3, 0xf3),
    glass_tint: 0.65,
};
pub const GREEN: D2D1_COLOR_F = rgb(0x30, 0xd1, 0x58);
pub const RED: D2D1_COLOR_F = rgb(0xff, 0x45, 0x3a);
pub const BLUE: D2D1_COLOR_F = rgb(0x0a, 0x84, 0xff);

pub const fn rgb(r: u8, g: u8, b: u8) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
}

/// Divider: text-colored smoke. Lighter than background in dark theme, darker in light.
/// Background color bleeds through locally, as there is no single "average" Mica color.
const DIVIDER_ALPHA: f32 = 0.12;

pub const fn black(a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a }
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub palette: Palette,
    /// DTW and search resting surface: taskbar tone, not accent. Always visible.
    pub surface: D2D1_COLOR_F,
    /// Interaction plates: accent tone for button and block hover.
    pub plate: D2D1_COLOR_F,
    /// Dropdown selection: same accent, slightly denser than plates.
    pub select: D2D1_COLOR_F,
    /// Enabled toggle: vivid taskbar tone.
    pub vivid: D2D1_COLOR_F,
    pub font: Option<String>,
}

impl Theme {
    pub fn from_config(c: &crate::config::Config, light: bool) -> Self {
        let palette = if light { LIGHT } else { DARK };
        let clear = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        let mut t = Self { palette, surface: clear, plate: clear, select: clear, vivid: clear, font: c.font.clone() };
        let body = [palette.body.r, palette.body.g, palette.body.b];
        t.apply_look(light, body, [0.5; 3]);
        t
    }

    pub fn apply_look(&mut self, light: bool, bg: crate::look::Rgb, accent: crate::look::Rgb) {
        self.palette = if light { LIGHT } else { DARK };
        let c = |c: D2D1_COLOR_F| [c.r, c.g, c.b];
        let p = super::tone::plate(accent, bg, c(self.palette.fg), c(self.palette.dim), light);
        self.plate = D2D1_COLOR_F { r: p.color[0], g: p.color[1], b: p.color[2], a: p.alpha };
        let s = super::tone::plate(bg, bg, c(self.palette.fg), c(self.palette.dim), light);
        self.surface = D2D1_COLOR_F { r: s.color[0], g: s.color[1], b: s.color[2], a: s.alpha };
        self.select = D2D1_COLOR_F { a: (p.alpha + SELECT_EXTRA).min(1.0), ..self.plate };
        let v = super::tone::vivid(bg, light);
        self.vivid = D2D1_COLOR_F { r: v[0], g: v[1], b: v[2], a: 1.0 };
    }
}

// ── Utilities ──

pub fn ease_out_cubic(p: f32) -> f32 {
    1.0 - (1.0 - p).powi(3)
}

pub fn ease_in_out(p: f32) -> f32 {
    if p < 0.5 { 4.0 * p * p * p } else { 1.0 - (-2.0 * p + 2.0).powi(3) / 2.0 }
}

/// Linearly interpolates `v` toward `target` by `step`. Returns `true` if still moving.
pub fn tween(v: &mut f32, target: f32, step: f32) -> bool {
    let moving = *v != target;
    if target > *v {
        *v = (*v + step).min(target);
    } else if target < *v {
        *v = (*v - step).max(target);
    }
    moving
}

pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub type Rect = (f32, f32, f32, f32);

pub fn inside((l, t, r, b): Rect, x: f32, y: f32) -> bool {
    x >= l && x <= r && y >= t && y <= b
}

// ── Text ──

struct Formats {
    text: IDWriteTextFormat,
    bold: IDWriteTextFormat,
    time: IDWriteTextFormat,
    date: IDWriteTextFormat,
    small: IDWriteTextFormat,
    tiny: IDWriteTextFormat,
    big: IDWriteTextFormat,
    symbol: IDWriteTextFormat,
    symbol_big: IDWriteTextFormat,
    mono: IDWriteTextFormat,
    input: IDWriteTextFormat,
}

static INTER: [&[u8]; 3] = [
    include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
];

fn pick_font(dw: &IDWriteFactory, custom: Option<&str>) -> (HSTRING, Option<IDWriteFontCollection>) {
    if let Some(name) = custom.filter(|n| installed(dw, n)) {
        return (name.into(), None);
    }
    match inter(dw) {
        Ok(c) => ("Inter".into(), Some(c)),
        Err(_) if installed(dw, "Segoe UI Variable Text") => ("Segoe UI Variable Text".into(), None),
        Err(_) => ("Segoe UI".into(), None),
    }
}

fn installed(dw: &IDWriteFactory, family: &str) -> bool {
    // SAFETY: DirectWrite calls on a live factory; the family HSTRING and out values outlive them.
    unsafe {
        let mut c = None;
        if dw.GetSystemFontCollection(&mut c, false).is_err() {
            return false;
        }
        let Some(c) = c else { return false };
        let (mut i, mut exists) = (0u32, BOOL(0));
        c.FindFamilyName(&HSTRING::from(family), &mut i, &mut exists).is_ok() && exists.as_bool()
    }
}

/// Collection from embedded Inter files. Data is not copied; files reference bytes directly
/// from the exe image. The owner is passed solely to prevent DirectWrite from copying the data.
fn inter(dw: &IDWriteFactory) -> Result<IDWriteFontCollection> {
    // SAFETY: DirectWrite calls on live COM objects; the font data is static, so it outlives the
    // font file references pointing into it.
    unsafe {
        let f5: IDWriteFactory5 = dw.cast()?;
        let loader = f5.CreateInMemoryFontFileLoader()?;
        f5.RegisterFontFileLoader(&loader)?;
        let owner: IUnknown = loader.cast()?;
        let builder = f5.CreateFontSetBuilder()?;
        for data in INTER {
            let file = loader.CreateInMemoryFontFileReference(dw, data.as_ptr().cast(), data.len() as u32, &owner)?;
            builder.AddFontFile(&file)?;
        }
        let set = builder.CreateFontSet()?;
        f5.CreateFontCollectionFromFontSet(&set)?.cast()
    }
}

impl Formats {
    fn new(dw: &IDWriteFactory, custom: Option<&str>) -> Result<Self> {
        let (family, collection) = pick_font(dw, custom);
        let locale = HSTRING::from(crate::i18n::t().locale);
        let up = if cjk() { CJK_UP } else { 0.0 };
        // SAFETY: DirectWrite calls on live COM objects; every argument outlives its call.
        let font = |weight: DWRITE_FONT_WEIGHT, size| unsafe {
            dw.CreateTextFormat(&family, collection.as_ref(), weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, &locale)
        };
        // SAFETY: DirectWrite calls on live COM objects; every argument outlives its call.
        let make = |family: PCWSTR, weight: DWRITE_FONT_WEIGHT, size| unsafe {
            dw.CreateTextFormat(family, None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, &locale)
        };
        Ok(Self {
            text: font(DWRITE_FONT_WEIGHT_NORMAL, 13.0)?,
            bold: font(DWRITE_FONT_WEIGHT_SEMI_BOLD, 13.0)?,
            time: font(DWRITE_FONT_WEIGHT_SEMI_BOLD, 14.0)?,
            date: font(DWRITE_FONT_WEIGHT_MEDIUM, 10.5 + up)?,
            small: font(DWRITE_FONT_WEIGHT_NORMAL, 12.0 + up)?,
            tiny: font(DWRITE_FONT_WEIGHT_NORMAL, 11.0 + up)?,
            big: font(DWRITE_FONT_WEIGHT_MEDIUM, 30.0)?,
            symbol: make(w!("Segoe UI Symbol"), DWRITE_FONT_WEIGHT_NORMAL, 14.0)?,
            symbol_big: make(w!("Segoe UI Symbol"), DWRITE_FONT_WEIGHT_NORMAL, 16.0)?,
            mono: make(w!("Consolas"), DWRITE_FONT_WEIGHT_NORMAL, 11.0)?,
            input: font(DWRITE_FONT_WEIGHT_NORMAL, 16.0)?,
        })
    }
}

#[derive(Clone)]
pub struct Text {
    pub layout: IDWriteTextLayout,
    pub w: f32,
    pub h: f32,
    pub baseline: f32,
}

fn baseline(layout: &IDWriteTextLayout) -> f32 {
    let mut lines = [DWRITE_LINE_METRICS::default()];
    let mut count = 0;
    // SAFETY: `lines` is writable for its length, and `count` is a valid out pointer.
    let _ = unsafe { layout.GetLineMetrics(Some(&mut lines), &mut count) };
    lines[0].baseline
}

fn text(dw: &IDWriteFactory, s: &str, fmt: &IDWriteTextFormat) -> Result<Text> {
    text_max(dw, s, fmt, 10_000.0)
}

fn text_wrap(dw: &IDWriteFactory, s: &str, fmt: &IDWriteTextFormat, max_w: f32) -> Result<Text> {
    let wide: Vec<u16> = s.encode_utf16().collect();
    // SAFETY: DirectWrite calls on live COM objects; every argument outlives its call.
    unsafe {
        let layout = dw.CreateTextLayout(&wide, fmt, max_w, 1_000.0)?;
        let mut m = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut m)?;
        let b = baseline(&layout);
        Ok(Text { layout, w: m.width, h: m.height, baseline: b })
    }
}

fn text_max(dw: &IDWriteFactory, s: &str, fmt: &IDWriteTextFormat, max_w: f32) -> Result<Text> {
    let wide: Vec<u16> = s.encode_utf16().collect();
    // SAFETY: DirectWrite calls on live COM objects; every argument outlives its call.
    unsafe {
        let layout = dw.CreateTextLayout(&wide, fmt, max_w, 1_000.0)?;
        layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        let sign = dw.CreateEllipsisTrimmingSign(fmt)?;
        layout.SetTrimming(&DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 }, &sign)?;
        measure(layout, max_w)
    }
}

fn measure(layout: IDWriteTextLayout, max_w: f32) -> Result<Text> {
    // SAFETY: DirectWrite calls on live COM objects; every argument outlives its call.
    unsafe {
        let mut m = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut m)?;
        let o = layout.GetOverhangMetrics()?;
        let ink = max_w + o.right;
        let w = m.widthIncludingTrailingWhitespace.max(ink).min(max_w);
        let b = baseline(&layout);
        Ok(Text { layout, w, h: m.height, baseline: b })
    }
}

fn tabular(dw: &IDWriteFactory, t: &Text, len: u32, case: bool) -> Result<()> {
    // SAFETY: DirectWrite calls on live COM objects; every argument outlives its call.
    unsafe {
        let typo = dw.CreateTypography()?;
        typo.AddFontFeature(DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, parameter: 1 })?;
        if case {
            typo.AddFontFeature(DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_CASE_SENSITIVE_FORMS, parameter: 1 })?;
        }
        t.layout.SetTypography(&typo, DWRITE_TEXT_RANGE { startPosition: 0, length: len })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Font {
    Text,
    Bold,
    Time,
    Date,
    Small,
    Tiny,
    Big,
    Symbol,
    SymbolBig,
    Input,
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style {
    Plain,
    Tabular { case: bool },
    Trim(u32),
    Title { from: u32 },
    Wrap(u32),
}

impl Style {
    pub fn trim(width: f32) -> Self {
        Style::Trim(width.max(0.0).to_bits())
    }

    pub fn wrap(width: f32) -> Self {
        Style::Wrap(width.max(1.0).to_bits())
    }
}

pub struct Texts {
    fmt: Formats,
    pub hinted: Option<windows::Win32::Graphics::DirectWrite::IDWriteRenderingParams>,
    map: HashMap<(String, Font, Style), (Text, u64)>,
    frame: u64,
}

const EVICT_FRAMES: u64 = 600;
const CJK_UP: f32 = 1.0;

fn cjk() -> bool {
    matches!(crate::i18n::t().locale, "zh-cn" | "ja-jp")
}

/// Pixel-snapped rendering parameters (GDI-style). The island draws on a transparent surface,
/// which allows only grayscale antialiasing (no ClearType); without snapping, the many horizontal
/// strokes of an 11 px CJK glyph blur into a gray blot. Latin text is fine without it, so this is
/// used only for Chinese and Japanese.
fn hinted(dw: &IDWriteFactory) -> Result<windows::Win32::Graphics::DirectWrite::IDWriteRenderingParams> {
    use windows::Win32::Graphics::DirectWrite::{DWRITE_PIXEL_GEOMETRY_FLAT, DWRITE_RENDERING_MODE_GDI_NATURAL};
    // SAFETY: DirectWrite calls on live COM objects; every argument outlives its call.
    unsafe {
        let base = dw.CreateRenderingParams()?;
        dw.CreateCustomRenderingParams(base.GetGamma(), base.GetEnhancedContrast(), 0.0, DWRITE_PIXEL_GEOMETRY_FLAT, DWRITE_RENDERING_MODE_GDI_NATURAL)
    }
}

impl Texts {
    pub fn new(dw: &IDWriteFactory, font: Option<&str>) -> Result<Self> {
        Ok(Self { fmt: Formats::new(dw, font)?, hinted: if cjk() { hinted(dw).ok() } else { None }, map: Default::default(), frame: 0 })
    }

    fn format(&self, f: Font) -> &IDWriteTextFormat {
        match f {
            Font::Text => &self.fmt.text,
            Font::Bold => &self.fmt.bold,
            Font::Time => &self.fmt.time,
            Font::Date => &self.fmt.date,
            Font::Small => &self.fmt.small,
            Font::Tiny => &self.fmt.tiny,
            Font::Big => &self.fmt.big,
            Font::Symbol => &self.fmt.symbol,
            Font::SymbolBig => &self.fmt.symbol_big,
            Font::Mono => &self.fmt.mono,
            Font::Input => &self.fmt.input,
        }
    }

    fn get(&mut self, dw: &IDWriteFactory, s: &str, f: Font, style: Style, dim: &ID2D1SolidColorBrush) -> Result<Text> {
        let key = (s.to_string(), f, style);
        if let Some((t, used)) = self.map.get_mut(&key) {
            *used = self.frame;
            return Ok(t.clone());
        }
        let fmt = self.format(f);
        let mut t = match style {
            Style::Trim(bits) => text_max(dw, s, fmt, f32::from_bits(bits))?,
            Style::Wrap(bits) => text_wrap(dw, s, fmt, f32::from_bits(bits))?,
            _ => text(dw, s, fmt)?,
        };
        let tracking = match f {
            Font::Time => -0.02 * 14.0,
            Font::Date => 0.06 * 10.5,
            _ => 0.0,
        };
        if tracking != 0.0 {
            let len = s.encode_utf16().count() as u32;
            let l1: IDWriteTextLayout1 = t.layout.cast()?;
            // SAFETY: DirectWrite call on a live layout with plain value arguments.
            unsafe { l1.SetCharacterSpacing(0.0, tracking, 0.0, DWRITE_TEXT_RANGE { startPosition: 0, length: len })? };
            t = measure(t.layout.clone(), 10_000.0)?;
        }
        match style {
            Style::Tabular { case } => {
                tabular(dw, &t, s.encode_utf16().count() as u32, case)?;
                t = measure(t.layout.clone(), 10_000.0)?;
            }
            // SAFETY: DirectWrite call on a live layout; `dim` is a live brush.
            Style::Title { from } => unsafe { t.layout.SetDrawingEffect(dim, DWRITE_TEXT_RANGE { startPosition: from, length: u32::MAX - from })? },
            _ => {}
        }
        self.map.insert(key, (t.clone(), self.frame));
        Ok(t)
    }

    pub fn height(&self, dw: &IDWriteFactory, s: &str, f: Font, wrap: f32) -> Result<f32> {
        Ok(text_wrap(dw, s, self.format(f), wrap)?.h)
    }

    pub fn clamp_lines(&self, dw: &IDWriteFactory, s: &str, f: Font, wrap: f32, max: usize) -> Result<String> {
        let fmt = self.format(f);
        let lines = |s: &str| -> Result<Vec<u32>> {
            let t = text_wrap(dw, s, fmt, wrap)?;
            let mut n = 0u32;
            // SAFETY: DirectWrite call on a live layout; `n` is a valid out pointer.
            let _ = unsafe { t.layout.GetLineMetrics(None, &mut n) };
            let mut buf = vec![DWRITE_LINE_METRICS::default(); n as usize];
            // SAFETY: `buf` holds exactly `n` line metrics, as the call above reported.
            unsafe { t.layout.GetLineMetrics(Some(&mut buf), &mut n)? };
            Ok(buf.iter().map(|l| l.length).collect())
        };
        let first = lines(s)?;
        if first.len() <= max {
            return Ok(s.to_string());
        }
        let wide: Vec<u16> = s.encode_utf16().collect();
        let mut end: usize = first[..max].iter().map(|&l| l as usize).sum::<usize>().min(wide.len());
        loop {
            let cut = String::from_utf16_lossy(&wide[..end]);
            let out = format!("{}…", cut.trim_end());
            if end == 0 || lines(&out)?.len() <= max {
                return Ok(out);
            }
            end -= 1;
        }
    }

    pub fn end_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        self.map.retain(|_, (_, used)| frame - *used < EVICT_FRAMES);
    }
}

// ── Brushes ──

/// Brushes are created once; opacity is modified in-place via `SetOpacity`.
pub struct Brushes {
    pub fg: ID2D1SolidColorBrush,
    pub dim: ID2D1SolidColorBrush,
    pub body: ID2D1SolidColorBrush,
    pub track: ID2D1SolidColorBrush,
    pub plate: ID2D1SolidColorBrush,
    pub surface: ID2D1SolidColorBrush,
    pub vivid: ID2D1SolidColorBrush,
    pub select: ID2D1SolidColorBrush,
    pub soft: ID2D1SolidColorBrush,
    pub green: ID2D1SolidColorBrush,
    pub red: ID2D1SolidColorBrush,
    pub blue: ID2D1SolidColorBrush,
    /// The taskbar edge color, as the dropdown outline: outlines of fields. Set every frame.
    pub edge: ID2D1SolidColorBrush,
}

impl Brushes {
    pub fn new(dc: &ID2D1DeviceContext, theme: &Theme) -> Result<Self> {
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        let b = |c: &D2D1_COLOR_F| unsafe { dc.CreateSolidColorBrush(c, None) };
        Ok(Self {
            plate: b(&theme.plate)?,
            surface: b(&theme.surface)?,
            vivid: b(&theme.vivid)?,
            select: b(&theme.select)?,
            fg: b(&theme.palette.fg)?,
            dim: b(&theme.palette.dim)?,
            body: b(&theme.palette.body)?,
            track: b(&theme.palette.track)?,
            soft: b(&theme.palette.soft)?,
            green: b(&GREEN)?,
            red: b(&RED)?,
            blue: b(&BLUE)?,
            edge: b(&theme.palette.dim)?,
        })
    }
}

pub fn set_opacity(b: &ID2D1SolidColorBrush, a: f32) {
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe { b.SetOpacity(a) };
}

pub fn draw_text(dc: &ID2D1DeviceContext, t: &Text, x: f32, y: f32, brush: &ID2D1SolidColorBrush) {
    // Vertical pixel snapping prevents blurring during fractional centering.
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe { dc.DrawTextLayout(Vector2 { X: x, Y: y.round() }, &t.layout, brush, D2D1_DRAW_TEXT_OPTIONS_NONE) };
}

fn s_curve(dc: &ID2D1DeviceContext, a0: f32, a1: f32) -> Result<ID2D1GradientStopCollection1> {
    const N: usize = 10;
    let stops: Vec<D2D1_GRADIENT_STOP> = (0..=N)
        .map(|i| {
            let t = i as f32 / N as f32;
            let s = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
            D2D1_GRADIENT_STOP { position: t, color: black(a0 + (a1 - a0) * s) }
        })
        .collect();
    gradient(dc, &stops)
}

pub fn linear_fade(dc: &ID2D1DeviceContext, from: f32, to: f32) -> Result<ID2D1LinearGradientBrush> {
    let stops = gradient(dc, &[D2D1_GRADIENT_STOP { position: 0.0, color: black(1.0) }, D2D1_GRADIENT_STOP { position: 1.0, color: black(0.0) }])?;
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        dc.CreateLinearGradientBrush(
            &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2 { X: 0.0, Y: from }, endPoint: Vector2 { X: 0.0, Y: to } },
            None,
            &stops,
        )
    }
}

pub fn vignette(dc: &ID2D1DeviceContext, cx: f32, cy: f32, rx: f32, ry: f32, rim: f32) -> Result<windows::Win32::Graphics::Direct2D::ID2D1RadialGradientBrush> {
    let stops = s_curve(dc, 0.0, rim)?;
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        dc.CreateRadialGradientBrush(
            &windows::Win32::Graphics::Direct2D::D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: Vector2 { X: cx, Y: cy },
                gradientOriginOffset: Vector2 { X: 0.0, Y: 0.0 },
                radiusX: rx,
                radiusY: ry,
            },
            None,
            &stops,
        )
    }
}

pub fn gradient(dc: &ID2D1DeviceContext, stops: &[D2D1_GRADIENT_STOP]) -> Result<ID2D1GradientStopCollection1> {
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        dc.CreateGradientStopCollection(
            stops,
            D2D1_COLOR_SPACE_SRGB,
            D2D1_COLOR_SPACE_SRGB,
            D2D1_BUFFER_PRECISION_8BPC_UNORM,
            D2D1_EXTEND_MODE_CLAMP,
            D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
        )
    }
}

// ── Hotspots ──

#[derive(Debug, Clone, Copy)]
pub struct Hotspot {
    pub rect: Rect,
    pub kind: HotKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HotKind {
    Media(Cmd),
    Dismiss,
    Note(super::Part),
    Launch(usize),
    LaunchAct(usize),
    Swatch(usize),
    OpenApp,
    MenuRow(usize),
    MenuBack,
    Seek { duration: f64 },
}

// ── Marquee ──

pub const MARQUEE_SPEED: f32 = 35.0;
pub const MARQUEE_GAP: f32 = 32.0;
pub const MARQUEE_FADE: f32 = 10.0;

// ── Frame Context ──

#[derive(Default)]
pub struct Glide {
    at: Option<(f32, f32, f32, f32)>,
    to: Option<(f32, f32, f32, f32)>,
    last_t: Option<f32>,
}

const GLIDE_TAU: f32 = 0.05;
const GLIDE_GAP: f32 = 0.25;

impl Glide {
    pub fn step(&mut self, target: Option<(f32, f32, f32, f32)>, t: f32) -> Option<(f32, f32, f32, f32)> {
        let dt = self.last_t.map_or(f32::MAX, |l| t - l);
        self.last_t = Some(t);
        self.to = target;
        self.at = match (self.at, target) {
            (Some(a), Some(b)) if dt < GLIDE_GAP => {
                let k = 1.0 - (-dt / GLIDE_TAU).exp();
                let m = |x: f32, y: f32| {
                    let v = x + (y - x) * k;
                    if (y - v).abs() < 0.3 { y } else { v }
                };
                Some((m(a.0, b.0), m(a.1, b.1), m(a.2, b.2), m(a.3, b.3)))
            }
            (_, b) => b,
        };
        self.at
    }

    pub fn moving(&self) -> bool {
        self.at != self.to
    }

    pub fn draw(&self, dc: &ID2D1DeviceContext, brush: &ID2D1SolidColorBrush, alpha: f32, radius: f32) {
        let Some((l, t, r, b)) = self.at else { return };
        set_opacity(brush, alpha);
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe {
            dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: l, top: t, right: r, bottom: b }, radiusX: radius, radiusY: radius }, brush)
        };
        set_opacity(brush, 1.0);
    }
}

const SELECT_EXTRA: f32 = 0.22;

pub struct EdgeFade {
    pub brush: windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush,
    pub seam: ID2D1LinearGradientBrush,
    pub vignette: windows::Win32::Graphics::Direct2D::ID2D1Brush,
    pub rect: D2D_RECT_F,
}

pub struct Frame<'a> {
    pub edge: Option<EdgeFade>,
    pub dither: Option<(windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush, f32)>,
    pub backdrop: [f32; 3],
    pub glass: bool,
    pub gfx: &'a Gfx,
    pub dc: &'a ID2D1DeviceContext,
    pub br: &'a Brushes,
    pub texts: &'a mut Texts,
    pub hotspots: &'a mut Vec<Hotspot>,
    /// Texts cut to fit, with their place: a tooltip shows them whole.
    pub cut: &'a mut Vec<(Rect, String)>,
    pub mouse: Option<(f32, f32)>,
    pub t: f32,
    pub scroll: &'a mut Option<super::scroller::Request>,
    pub app_icons: &'a mut std::collections::HashMap<String, Option<windows::Win32::Graphics::Direct2D::ID2D1Bitmap1>>,
    pub running: std::cell::Cell<bool>,
}

impl Frame<'_> {
    /// Remembers `text` drawn in `rect` if it does not fit in `room`: its tooltip reads it whole
    /// instead of waiting for a marquee.
    pub fn cut_if_long(&mut self, rect: Rect, text: &str, font: Font, room: f32) -> Result<()> {
        if !text.is_empty() && self.txt(text, font, Style::Plain)?.w > room + 0.5 {
            self.cut.push((rect, text.to_string()));
        }
        Ok(())
    }

    pub fn shade(&self, a: f32) -> Result<ID2D1SolidColorBrush> {
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        let c = D2D1_COLOR_F { a: a * DIVIDER_ALPHA, ..unsafe { self.br.fg.GetColor() } };
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe { self.dc.CreateSolidColorBrush(&c, None) }
    }

    pub fn draw_edge(&self, vignette: bool) {
        let Some(e) = &self.edge else { return };
        self.fill_edge(e, e.seam.clone().into());
        if vignette {
            self.fill_edge(e, e.vignette.clone());
        }
        if let Some((tile, a)) = &self.dither {
            // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
            // outlives the call.
            unsafe {
                tile.SetOpacity(*a);
                self.dc.FillRectangle(&e.rect, tile);
            }
        }
    }

    fn fill_edge(&self, e: &EdgeFade, fade: windows::Win32::Graphics::Direct2D::ID2D1Brush) {
        let params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: e.rect,
            geometricMask: std::mem::ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: std::mem::ManuallyDrop::new(Some(fade)),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        // SAFETY: Direct2D calls on live COM objects; `params` and its mask outlive the calls.
        unsafe {
            self.dc.PushLayer(&params, None);
            self.dc.FillRectangle(&e.rect, &e.brush);
            self.dc.PopLayer();
        }
        drop(std::mem::ManuallyDrop::into_inner(params.opacityBrush));
    }

    pub fn txt(&mut self, s: &str, f: Font, style: Style) -> Result<Text> {
        self.texts.get(&self.gfx.dwrite, s, f, style, &self.br.dim)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn marquee(&self, text: &Text, l: f32, r: f32, cy: f32, brush: &ID2D1SolidColorBrush, alpha: f32, since: f32, delay: f32) -> Result<()> {
        let dc = self.dc;
        let y = cy - text.h / 2.0;
        let room = r - l;
        if text.w <= room {
            draw_text(dc, text, l, y, brush);
            return Ok(());
        }
        self.running.set(true);
        let run = (self.t - since - delay).max(0.0);
        let cycle = text.w + MARQUEE_GAP;
        let off = (run * MARQUEE_SPEED) % cycle;
        let left_fade = (run * 4.0).min(1.0);
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe {
            let f = MARQUEE_FADE / room;
            let coll = gradient(
                dc,
                &[
                    D2D1_GRADIENT_STOP { position: 0.0, color: black(alpha * (1.0 - left_fade)) },
                    D2D1_GRADIENT_STOP { position: f, color: black(alpha) },
                    D2D1_GRADIENT_STOP { position: 1.0 - f, color: black(alpha) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: black(0.0) },
                ],
            )?;
            let mask = dc.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2 { X: 0.0, Y: 0.0 }, endPoint: Vector2 { X: room, Y: 0.0 } },
                None,
                &coll,
            )?;
            mask.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: l, M32: 0.0 });
            let params = D2D1_LAYER_PARAMETERS1 {
                contentBounds: D2D_RECT_F { left: l, top: y - 2.0, right: r, bottom: y + text.h + 2.0 },
                geometricMask: std::mem::ManuallyDrop::new(None),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: std::mem::ManuallyDrop::new(Some(mask.cast()?)),
                layerOptions: D2D1_LAYER_OPTIONS1_NONE,
            };
            brush.SetOpacity(1.0);
            dc.PushLayer(&params, None);
            if self.texts.hinted.is_some() {
                dc.SetTextRenderingParams(None);
            }
            draw_text(dc, text, l - off, y, brush);
            draw_text(dc, text, l - off + cycle, y, brush);
            if let Some(p) = &self.texts.hinted {
                dc.SetTextRenderingParams(p);
            }
            dc.PopLayer();
            drop(std::mem::ManuallyDrop::into_inner(params.opacityBrush));
            brush.SetOpacity(alpha);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn button(&mut self, icon: Icon, kind: HotKind, cx: f32, cy: f32, size: f32, radius: f32, brush: &ID2D1SolidColorBrush, alpha: f32) -> Result<()> {
        let rect = (cx - radius, cy - radius, cx + radius, cy + radius);
        if self.mouse.is_some_and(|(mx, my)| inside(rect, mx, my)) && alpha > 0.5 {
            // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow
            // that outlives the call.
            unsafe {
                self.br.plate.SetOpacity(alpha);
                self.dc.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: radius, radiusY: radius }, &self.br.plate);
                self.br.plate.SetOpacity(1.0);
            }
        }
        icons::draw(self.gfx, self.dc, icon, cx, cy, size, brush)?;
        if alpha > 0.5 {
            self.hotspots.push(Hotspot { rect, kind });
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn progress(&mut self, m: &media::State, l: f32, r: f32, cy: f32, thick: f32, fg: &ID2D1SolidColorBrush, alpha: f32) {
        if r <= l {
            return;
        }
        let frac = if m.duration > 0.0 { (m.position() / m.duration) as f32 } else { 0.0 };
        let rr = thick / 2.0;
        let bar = |right: f32| D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: l, top: cy - rr, right, bottom: cy + rr }, radiusX: rr, radiusY: rr };
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe {
            self.br.track.SetOpacity(alpha);
            self.dc.FillRoundedRectangle(&bar(r), &self.br.track);
            if frac > 0.0 {
                self.dc.FillRoundedRectangle(&bar(l + (r - l) * frac), fg);
            }
        }
        if alpha > 0.5 && m.duration > 0.0 {
            self.hotspots.push(Hotspot { rect: (l, cy - 8.0, r, cy + 8.0), kind: HotKind::Seek { duration: m.duration } });
        }
    }
}

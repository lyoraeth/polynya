// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vector icons on a 24×24 grid, scaled at draw time.
//!
//! Player buttons are filled polygons; weather and utility glyphs are strokes with round caps.

use windows::{
    Win32::Graphics::Direct2D::{
        Common::{D2D_SIZE_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN},
        D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_ELLIPSE, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
        ID2D1DeviceContext, ID2D1Factory1, ID2D1GeometrySink, ID2D1PathGeometry, ID2D1SolidColorBrush,
    },
    core::Result,
};
use windows_numerics::Vector2;

use crate::{gfx::Gfx, weather::Sky};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    /// Close cross.
    Close,
    /// Caps Lock / Num Lock arrow.
    Key,
    /// Check mark: done.
    Ok,
    /// Error cross: failed.
    Fail,
    /// "i" in a circle: a notification with no special meaning.
    Info,
    /// Claude Code, drawn with the glyph of its indicator, see `island::logo`.
    Claude,
    /// Other AI agents, drawn with dots, see `island::logo`.
    Agent,
    /// Magnifier: search.
    Search,
    /// Equals sign: calculator.
    Equals,
    /// Color dot, drawn elsewhere as a fill of that color.
    Color([u8; 3]),
}

/// Stroke polyline: points and whether it is closed.
type Path = (&'static [(f32, f32)], bool);

impl Icon {
    /// Name for the command line: `ok`, `fail`, `info`, `claude`, `agent`, `key`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ok" => Some(Icon::Ok),
            "fail" => Some(Icon::Fail),
            "info" => Some(Icon::Info),
            "claude" => Some(Icon::Claude),
            "agent" => Some(Icon::Agent),
            "key" => Some(Icon::Key),
            _ => s.strip_prefix("color:").and_then(proto::parse_hex).map(Icon::Color),
        }
    }

    /// Stroke icons: width in grid units and polylines.
    fn strokes(self) -> Option<(f32, &'static [Path])> {
        match self {
            Icon::Close => Some((4.0, &[(&[(4.0, 4.0), (20.0, 20.0)], false), (&[(20.0, 4.0), (4.0, 20.0)], false)])),
            Icon::Key => Some((
                2.0,
                &[
                    (&[(12.0, 4.0), (19.0, 12.0), (15.0, 12.0), (15.0, 16.0), (9.0, 16.0), (9.0, 12.0), (5.0, 12.0)], true),
                    (&[(9.0, 20.0), (15.0, 20.0)], false),
                ],
            )),
            Icon::Ok => Some((2.4, &[(&[(5.0, 12.5), (9.5, 17.0), (19.0, 7.5)], false)])),
            Icon::Fail => Some((2.4, &[(&[(6.5, 6.5), (17.5, 17.5)], false), (&[(17.5, 6.5), (6.5, 17.5)], false)])),
            Icon::Equals => Some((2.2, &[(&[(6.0, 9.0), (18.0, 9.0)], false), (&[(6.0, 15.0), (18.0, 15.0)], false)])),
            _ => None,
        }
    }

    fn polygons(self) -> &'static [&'static [(f32, f32)]] {
        match self {
            Icon::Play => &[&[(8.0, 5.0), (19.0, 12.0), (8.0, 19.0)]],
            Icon::Pause => &[&[(7.0, 5.0), (11.0, 5.0), (11.0, 19.0), (7.0, 19.0)], &[(13.0, 5.0), (17.0, 5.0), (17.0, 19.0), (13.0, 19.0)]],
            Icon::Prev => &[&[(6.0, 5.0), (8.0, 5.0), (8.0, 19.0), (6.0, 19.0)], &[(20.0, 5.0), (20.0, 19.0), (9.0, 12.0)]],
            Icon::Next => &[&[(16.0, 5.0), (18.0, 5.0), (18.0, 19.0), (16.0, 19.0)], &[(4.0, 5.0), (15.0, 12.0), (4.0, 19.0)]],
            _ => &[],
        }
    }
}

/// Maps 24×24 grid coordinates to window coordinates.
#[derive(Clone, Copy)]
struct Grid {
    ox: f32,
    oy: f32,
    s: f32,
}

impl Grid {
    fn new(cx: f32, cy: f32, size: f32) -> Self {
        Self { ox: cx - size / 2.0, oy: cy - size / 2.0, s: size / 24.0 }
    }

    fn p(&self, x: f32, y: f32) -> Vector2 {
        Vector2 { X: self.ox + x * self.s, Y: self.oy + y * self.s }
    }
}

/// Draws an icon of `size` centered at `(cx, cy)`.
pub fn draw(gfx: &Gfx, dc: &ID2D1DeviceContext, icon: Icon, cx: f32, cy: f32, size: f32, brush: &ID2D1SolidColorBrush) -> Result<()> {
    let g = Grid::new(cx, cy, size);
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        let geo = gfx.d2d.CreatePathGeometry()?;
        let sink = geo.Open()?;
        match icon {
            Icon::Search => {
                sink.BeginFigure(g.p(15.2, 15.2), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(g.p(20.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.Close()?;
                dc.DrawGeometry(&geo, brush, 2.2 * g.s, &gfx.round);
                dc.DrawEllipse(&D2D1_ELLIPSE { point: g.p(10.5, 10.5), radiusX: 6.5 * g.s, radiusY: 6.5 * g.s }, brush, 2.2 * g.s, &gfx.round);
                return Ok(());
            }
            Icon::Info => {
                sink.BeginFigure(g.p(12.0, 11.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(g.p(12.0, 16.5));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.Close()?;
                dc.DrawGeometry(&geo, brush, 2.0 * g.s, &gfx.round);
                dc.DrawEllipse(&D2D1_ELLIPSE { point: g.p(12.0, 12.0), radiusX: 9.0 * g.s, radiusY: 9.0 * g.s }, brush, 2.0 * g.s, &gfx.round);
                dc.FillEllipse(&D2D1_ELLIPSE { point: g.p(12.0, 7.8), radiusX: 1.3 * g.s, radiusY: 1.3 * g.s }, brush);
                return Ok(());
            }
            _ => {}
        }
        if let Some((width, paths)) = icon.strokes() {
            for &(pts, closed) in paths {
                sink.BeginFigure(g.p(pts[0].0, pts[0].1), D2D1_FIGURE_BEGIN_HOLLOW);
                for &(x, y) in &pts[1..] {
                    sink.AddLine(g.p(x, y));
                }
                sink.EndFigure(if closed { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
            }
            sink.Close()?;
            dc.DrawGeometry(&geo, brush, width * g.s, &gfx.round);
            return Ok(());
        }
        for poly in icon.polygons() {
            sink.BeginFigure(g.p(poly[0].0, poly[0].1), D2D1_FIGURE_BEGIN_FILLED);
            for &(x, y) in &poly[1..] {
                sink.AddLine(g.p(x, y));
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        }
        sink.Close()?;
        dc.FillGeometry(&geo, brush, None);
    }
    Ok(())
}

fn arc(sink: &ID2D1GeometrySink, g: &Grid, x: f32, y: f32, r: f32, clockwise: bool, large: bool) {
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: g.p(x, y),
            size: D2D_SIZE_F { width: r * g.s, height: r * g.s },
            rotationAngle: 0.0,
            sweepDirection: if clockwise { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
            arcSize: if large { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
        });
    }
}

/// Cloud outline; `dy` shifts it up to make room for precipitation.
fn cloud(d2d: &ID2D1Factory1, g: &Grid, dx: f32, dy: f32) -> Result<ID2D1PathGeometry> {
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        let geo: ID2D1PathGeometry = d2d.CreatePathGeometry()?.into();
        let sink = geo.Open()?;
        sink.BeginFigure(g.p(7.0 + dx, 18.0 + dy), D2D1_FIGURE_BEGIN_FILLED);
        sink.AddLine(g.p(17.0 + dx, 18.0 + dy));
        arc(&sink, g, 17.5 + dx, 10.03 + dy, 4.0, false, false);
        arc(&sink, g, 6.0 + dx, 11.0 + dy, 6.0, false, false);
        arc(&sink, g, 7.0 + dx, 18.0 + dy, 3.6, false, false);
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
        Ok(geo)
    }
}

/// A line segment between two points of the icon grid.
type Seg = ((f32, f32), (f32, f32));

/// Weather icon centered at `(cx, cy)`. `bg` fills the cloud inside so the sun behind
/// does not show through the outline.
#[allow(clippy::too_many_arguments)]
pub fn weather(
    gfx: &Gfx,
    dc: &ID2D1DeviceContext,
    sky: Sky,
    day: bool,
    cx: f32,
    cy: f32,
    size: f32,
    fg: &ID2D1SolidColorBrush,
    bg: &ID2D1SolidColorBrush,
) -> Result<()> {
    let g = Grid::new(cx, cy, size);
    let width = 1.8 * g.s;
    let (d2d, style) = (&gfx.d2d, &gfx.round);
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        // Open strokes: sun rays, precipitation, fog.
        let lines = |segs: &[Seg]| -> Result<()> {
            let geo = d2d.CreatePathGeometry()?;
            let sink = geo.Open()?;
            for &((x1, y1), (x2, y2)) in segs {
                sink.BeginFigure(g.p(x1, y1), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(g.p(x2, y2));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            sink.Close()?;
            dc.DrawGeometry(&geo, fg, width, style);
            Ok(())
        };
        let sun = |x: f32, y: f32, r: f32, ray_in: f32, ray_out: f32, rays: &[usize]| -> Result<()> {
            dc.DrawEllipse(&D2D1_ELLIPSE { point: g.p(x, y), radiusX: r * g.s, radiusY: r * g.s }, fg, width, style);
            let segs: Vec<_> = rays
                .iter()
                .map(|&i| {
                    let a = i as f32 * std::f32::consts::FRAC_PI_4;
                    ((x + a.cos() * ray_in, y + a.sin() * ray_in), (x + a.cos() * ray_out, y + a.sin() * ray_out))
                })
                .collect();
            lines(&segs)
        };
        let moon = |dx: f32, dy: f32| -> Result<()> {
            let geo = d2d.CreatePathGeometry()?;
            let sink = geo.Open()?;
            sink.BeginFigure(g.p(14.5 + dx, 4.5 + dy), D2D1_FIGURE_BEGIN_HOLLOW);
            arc(&sink, &g, 19.5 + dx, 14.5 + dy, 8.0, false, true);
            arc(&sink, &g, 14.5 + dx, 4.5 + dy, 6.2, true, false);
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            dc.DrawGeometry(&geo, fg, width, style);
            Ok(())
        };
        let draw_cloud = |dx: f32, dy: f32| -> Result<()> {
            let c = cloud(d2d, &g, dx, dy)?;
            dc.FillGeometry(&c, bg, None);
            dc.DrawGeometry(&c, fg, width, style);
            Ok(())
        };

        match sky {
            Sky::Clear if day => sun(12.0, 12.0, 4.0, 7.0, 9.5, &[0, 1, 2, 3, 4, 5, 6, 7])?,
            Sky::Clear => moon(0.0, 0.0)?,
            Sky::PartlyCloudy => {
                // Behind the cloud: a small sun or moon in the top left corner.
                if day {
                    sun(8.5, 8.0, 3.0, 5.0, 6.5, &[3, 4, 5, 6, 7])?;
                } else {
                    let small = Grid::new(cx - 3.5 * g.s, cy - 4.0 * g.s, size * 0.6);
                    let geo = d2d.CreatePathGeometry()?;
                    let sink = geo.Open()?;
                    sink.BeginFigure(small.p(14.5, 4.5), D2D1_FIGURE_BEGIN_HOLLOW);
                    arc(&sink, &small, 19.5, 14.5, 8.0, false, true);
                    arc(&sink, &small, 14.5, 4.5, 6.2, true, false);
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    sink.Close()?;
                    dc.DrawGeometry(&geo, fg, width, style);
                }
                draw_cloud(1.5, 2.0)?;
            }
            Sky::Cloudy => draw_cloud(0.0, 0.0)?,
            Sky::Fog => lines(&[((5.0, 8.0), (19.0, 8.0)), ((3.0, 12.0), (17.0, 12.0)), ((7.0, 16.0), (21.0, 16.0))])?,
            Sky::Rain => {
                draw_cloud(0.0, -3.0)?;
                lines(&[((9.0, 18.0), (8.0, 21.0)), ((13.0, 18.0), (12.0, 21.0)), ((17.0, 18.0), (16.0, 21.0))])?;
            }
            Sky::Snow => {
                draw_cloud(0.0, -3.0)?;
                for (x, y) in [(9.0, 19.0), (13.0, 21.0), (17.0, 19.0)] {
                    dc.FillEllipse(&D2D1_ELLIPSE { point: g.p(x, y), radiusX: 1.2 * g.s, radiusY: 1.2 * g.s }, fg);
                }
            }
            Sky::Thunder => {
                draw_cloud(0.0, -3.0)?;
                let geo = d2d.CreatePathGeometry()?;
                let sink = geo.Open()?;
                sink.BeginFigure(g.p(13.5, 15.5), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(g.p(11.0, 19.0));
                sink.AddLine(g.p(14.0, 19.0));
                sink.AddLine(g.p(11.5, 22.5));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.Close()?;
                dc.DrawGeometry(&geo, fg, width, style);
            }
        }
    }
    Ok(())
}

/// Draws an icon from an SVG path on a 24×24 grid with a 2-unit round-capped stroke, the Lucide
/// icon format, so module icons look like built-in ones. Several paths are separated by `|`: the
/// first `m` of each is absolute, as in a separate `<path>`.
pub fn draw_path(gfx: &Gfx, dc: &ID2D1DeviceContext, d: &str, cx: f32, cy: f32, size: f32, brush: &ID2D1SolidColorBrush) -> Result<()> {
    let g = Grid::new(cx, cy, size);
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        let geo = gfx.d2d.CreatePathGeometry()?;
        let sink = geo.Open()?;
        for part in d.split('|') {
            svg::trace(&sink, part, &|x, y| g.p(x, y), g.s, false);
        }
        sink.Close()?;
        dc.DrawGeometry(&geo, brush, 2.0 * g.s, &gfx.round);
    }
    Ok(())
}

/// Filled shape from an SVG path; `p` maps its coordinates to the window, `s` is the scale.
pub fn fill_geometry(gfx: &Gfx, d: &str, p: &dyn Fn(f32, f32) -> Vector2, s: f32) -> Result<windows::Win32::Graphics::Direct2D::ID2D1PathGeometry1> {
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        let geo = gfx.d2d.CreatePathGeometry()?;
        let sink = geo.Open()?;
        svg::trace(&sink, d, p, s, true);
        sink.Close()?;
        Ok(geo)
    }
}

/// Parses an SVG path (`M L H V C S Q T A Z`, lowercase is relative) into Direct2D geometry.
mod svg {
    use windows::Win32::Graphics::Direct2D::{
        Common::{D2D_SIZE_F, D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN},
        D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_QUADRATIC_BEZIER_SEGMENT, D2D1_SWEEP_DIRECTION_CLOCKWISE,
        D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, ID2D1GeometrySink,
    };
    use windows_numerics::Vector2;

    /// Path tokens: command letters and numbers (`-`, a dot and an exponent separate numbers without spaces).
    pub fn tokens(d: &str) -> Vec<Tok> {
        let mut out = Vec::new();
        let b = d.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i] as char;
            if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
                out.push(Tok::Cmd(c));
                i += 1;
            } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
                let start = i;
                i += 1;
                let mut dot = c == '.';
                while i < b.len() {
                    let c = b[i] as char;
                    if c.is_ascii_digit() {
                        i += 1;
                    } else if c == '.' && !dot {
                        dot = true;
                        i += 1;
                    } else if (c == 'e' || c == 'E') && i + 1 < b.len() {
                        i += 1;
                        if b[i] == b'-' || b[i] == b'+' {
                            i += 1;
                        }
                    } else {
                        break;
                    }
                }
                if let Ok(v) = d[start..i].parse() {
                    out.push(Tok::Num(v));
                }
            } else {
                i += 1;
            }
        }
        out
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Tok {
        Cmd(char),
        Num(f32),
    }

    /// Walks the path and writes it to `sink`; `p` maps grid coordinates to the window, `s` is the scale.
    pub fn trace(sink: &ID2D1GeometrySink, d: &str, p: &dyn Fn(f32, f32) -> Vector2, s: f32, filled: bool) {
        let toks = tokens(d);
        let mut i = 0;
        let (mut cur, mut start) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32));
        // The previous curve's second control point, for S and T.
        let mut last_ctrl: Option<(char, (f32, f32))> = None;
        let mut open = false;
        let mut cmd = 'M';
        let mut first = true;
        let num = |i: &mut usize| -> Option<f32> {
            match toks.get(*i) {
                Some(Tok::Num(v)) => {
                    *i += 1;
                    Some(*v)
                }
                _ => None,
            }
        };
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe {
            while i < toks.len() {
                if let Tok::Cmd(c) = toks[i] {
                    cmd = c;
                    i += 1;
                    if c == 'Z' || c == 'z' {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            open = false;
                        }
                        cur = start;
                        last_ctrl = None;
                        continue;
                    }
                } else if i == 0 {
                    break;
                }
                // The first `m` of a path is absolute.
                let rel = cmd.is_ascii_lowercase() && !(first && cmd == 'm');
                let base = if rel { cur } else { (0.0, 0.0) };
                let at = |x: f32, y: f32| (base.0 + x, base.1 + y);
                match cmd.to_ascii_uppercase() {
                    'M' => {
                        let (Some(x), Some(y)) = (num(&mut i), num(&mut i)) else { break };
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                        }
                        cur = at(x, y);
                        start = cur;
                        sink.BeginFigure(p(cur.0, cur.1), if filled { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW });
                        open = true;
                        // Further pairs after M are L.
                        cmd = if cmd == 'm' { 'l' } else { 'L' };
                        last_ctrl = None;
                    }
                    'L' => {
                        let (Some(x), Some(y)) = (num(&mut i), num(&mut i)) else { break };
                        cur = at(x, y);
                        sink.AddLine(p(cur.0, cur.1));
                        last_ctrl = None;
                    }
                    'H' => {
                        let Some(x) = num(&mut i) else { break };
                        cur = (if rel { cur.0 + x } else { x }, cur.1);
                        sink.AddLine(p(cur.0, cur.1));
                        last_ctrl = None;
                    }
                    'V' => {
                        let Some(y) = num(&mut i) else { break };
                        cur = (cur.0, if rel { cur.1 + y } else { y });
                        sink.AddLine(p(cur.0, cur.1));
                        last_ctrl = None;
                    }
                    'C' | 'S' => {
                        let c1 = if cmd.eq_ignore_ascii_case(&'C') {
                            let (Some(x), Some(y)) = (num(&mut i), num(&mut i)) else { break };
                            at(x, y)
                        } else {
                            match last_ctrl {
                                Some(('C', c)) => (2.0 * cur.0 - c.0, 2.0 * cur.1 - c.1),
                                _ => cur,
                            }
                        };
                        let (Some(x2), Some(y2), Some(x), Some(y)) = (num(&mut i), num(&mut i), num(&mut i), num(&mut i)) else { break };
                        let (c2, end) = (at(x2, y2), at(x, y));
                        sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: p(c1.0, c1.1), point2: p(c2.0, c2.1), point3: p(end.0, end.1) });
                        cur = end;
                        last_ctrl = Some(('C', c2));
                    }
                    'Q' | 'T' => {
                        let c1 = if cmd.eq_ignore_ascii_case(&'Q') {
                            let (Some(x), Some(y)) = (num(&mut i), num(&mut i)) else { break };
                            at(x, y)
                        } else {
                            match last_ctrl {
                                Some(('Q', c)) => (2.0 * cur.0 - c.0, 2.0 * cur.1 - c.1),
                                _ => cur,
                            }
                        };
                        let (Some(x), Some(y)) = (num(&mut i), num(&mut i)) else { break };
                        let end = at(x, y);
                        sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: p(c1.0, c1.1), point2: p(end.0, end.1) });
                        cur = end;
                        last_ctrl = Some(('Q', c1));
                    }
                    'A' => {
                        let (Some(rx), Some(ry), Some(rot), Some(large), Some(sweep), Some(x), Some(y)) =
                            (num(&mut i), num(&mut i), num(&mut i), num(&mut i), num(&mut i), num(&mut i), num(&mut i))
                        else {
                            break;
                        };
                        let end = at(x, y);
                        sink.AddArc(&D2D1_ARC_SEGMENT {
                            point: p(end.0, end.1),
                            size: D2D_SIZE_F { width: rx * s, height: ry * s },
                            rotationAngle: rot,
                            // In SVG y grows downward, as in the window: flag 1 is clockwise.
                            sweepDirection: if sweep != 0.0 { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
                            arcSize: if large != 0.0 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                        });
                        cur = end;
                        last_ctrl = None;
                    }
                    _ => break,
                }
                first = false;
            }
            if open {
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn numbers_without_separators() {
            let t = tokens("m15 6 3.4-3.4a2.1 2.1 0 1 1 3 3l.4.4Z");
            let nums: Vec<f32> = t.iter().filter_map(|t| if let Tok::Num(v) = t { Some(*v) } else { None }).collect();
            assert_eq!(nums, vec![15.0, 6.0, 3.4, -3.4, 2.1, 2.1, 0.0, 1.0, 1.0, 3.0, 3.0, 0.4, 0.4]);
            assert_eq!(t.last(), Some(&Tok::Cmd('Z')));
        }
    }
}

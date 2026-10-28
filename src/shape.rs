// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parametric island shape.
//!
//! At rest the island is a pill within the taskbar height. When it expands, a shape "flows" out
//! of the taskbar's bottom edge: concave fillets at the seam and regular corners below. It is one
//! shape with width and depth parameters: at zero depth there is no flow, so morphing is just
//! animating numbers, with no switching between shapes.

/// Radius of the concave fillet at the seam with the taskbar.
pub const FILLET: f32 = 12.0;
/// Radius of the flow's bottom corners.
pub const RADIUS: f32 = 22.0;

/// Seam and bottom corner radii for the given flow width and depth.
///
/// The radii are limited by the available depth and width, otherwise the outline self-intersects.
pub fn radii(w: f32, d: f32) -> (f32, f32) {
    let f = FILLET.min(d * 0.5).max(0.0);
    let r = RADIUS.min(d - f).min(w / 2.0).max(0.0);
    (f, r)
}

/// Outline segment: a line or a circular arc to the given point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Line(f32, f32),
    Arc { x: f32, y: f32, r: f32, clockwise: bool },
}

/// Closed outline of the flow.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub start: (f32, f32),
    pub segs: Vec<Seg>,
}

/// Outline of the flow under the taskbar.
///
/// `cx` and `w` are the flow's center and width, `d` its depth, `y0` the taskbar's bottom edge.
/// Returns `None` if the depth is too small for the flow to be visible.
pub fn flow(cx: f32, w: f32, d: f32, y0: f32, seam: f32) -> Option<Outline> {
    if d < 0.5 {
        return None;
    }
    let (l, r_edge) = (cx - w / 2.0, cx + w / 2.0);
    let (f, r) = radii(w, d);
    // The top of the outline reaches `seam` into the taskbar so antialiasing leaves no gap at
    // the seam; on a translucent taskbar the overlap shows as a strip, so there it is zero.
    let segs = vec![
        Seg::Line(l - f, y0),
        Seg::Arc { x: l, y: y0 + f, r: f, clockwise: true },
        Seg::Line(l, y0 + d - r),
        Seg::Arc { x: l + r, y: y0 + d, r, clockwise: false },
        Seg::Line(r_edge - r, y0 + d),
        Seg::Arc { x: r_edge, y: y0 + d - r, r, clockwise: false },
        Seg::Line(r_edge, y0 + f),
        Seg::Arc { x: r_edge + f, y: y0, r: f, clockwise: true },
        Seg::Line(r_edge + f, y0 - seam),
    ];
    Some(Outline { start: (l - f, y0 - seam), segs })
}

/// Flow bounds including the concave fillets: `(left, top, right, bottom)`.
pub fn flow_bounds(cx: f32, w: f32, d: f32, y0: f32) -> (f32, f32, f32, f32) {
    let (f, _) = radii(w, d);
    (cx - w / 2.0 - f, y0 - 1.0, cx + w / 2.0 + f, y0 + d)
}

impl Outline {
    /// Mirrors around the taskbar edge `y0`: for a bottom taskbar the flow grows upward.
    /// Mirroring reverses the arcs' direction.
    pub fn mirror(&self, y0: f32) -> Outline {
        let my = |y: f32| 2.0 * y0 - y;
        let segs = self
            .segs
            .iter()
            .map(|s| match *s {
                Seg::Line(x, y) => Seg::Line(x, my(y)),
                Seg::Arc { x, y, r, clockwise } => Seg::Arc { x, y: my(y), r, clockwise: !clockwise },
            })
            .collect();
        Outline { start: (self.start.0, my(self.start.1)), segs }
    }
}

/// Bounds mirrored around the edge `y0`, the counterpart of `Outline::mirror`.
pub fn mirror_bounds((l, t, r, b): (f32, f32, f32, f32), y0: f32) -> (f32, f32, f32, f32) {
    (l, 2.0 * y0 - b, r, 2.0 * y0 - t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_grows_up() {
        let o = flow(100.0, 200.0, 120.0, 400.0, 1.0).unwrap().mirror(400.0);
        assert_eq!(o.start, (100.0 - 100.0 - FILLET, 401.0));
        assert!(o.segs.iter().all(|s| match *s {
            Seg::Line(_, y) | Seg::Arc { y, .. } => y <= 401.0,
        }));
        assert_eq!(mirror_bounds(flow_bounds(100.0, 200.0, 120.0, 400.0), 400.0).1, 280.0);
    }

    #[test]
    fn zero_depth_has_no_outline() {
        assert!(flow(100.0, 200.0, 0.0, 36.0, 1.0).is_none());
    }

    #[test]
    fn radii_never_exceed_depth() {
        for d in [1.0, 5.0, 20.0, 40.0, 200.0] {
            let (f, r) = radii(300.0, d);
            assert!(f + r <= d + 1e-4, "d={d} f={f} r={r}");
        }
    }

    #[test]
    fn full_radii_at_large_depth() {
        assert_eq!(radii(300.0, 120.0), (FILLET, RADIUS));
    }

    #[test]
    fn outline_closes_at_panel_edge() {
        let o = flow(100.0, 200.0, 120.0, 36.0, 1.0).unwrap();
        let last = o.segs.last().copied().unwrap();
        assert_eq!(last, Seg::Line(100.0 + 100.0 + FILLET, 35.0));
        assert_eq!(o.start, (100.0 - 100.0 - FILLET, 35.0));
    }
}

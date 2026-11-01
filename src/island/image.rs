// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared drawing for module images: D2D bitmap caching, cover crop and contain fit.

use windows::{
    Win32::Graphics::Direct2D::{
        Common::D2D_RECT_F, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
        D2D1_ROUNDED_RECT, ID2D1Bitmap1,
    },
    core::{Interface, Result},
};
use windows_numerics::Matrix3x2;

use super::kit::Frame;
use crate::images;
use proto::ImageRef;

pub const THUMB: f32 = 56.0;
pub const THUMB_GAP: f32 = 12.0;
pub const MAX_FULL_H: f32 = 360.0;

pub struct Bitmap {
    raster: usize,
    bitmap: ID2D1Bitmap1,
}

/// Returns the current D2D bitmap, creating it only when the decoded raster changes.
pub fn sync(f: &Frame, image: &ImageRef, max_width: u32, cache: &mut Option<Bitmap>) -> Option<(ID2D1Bitmap1, u32, u32)> {
    let raster = images::get(image, max_width)?;
    let ptr = std::sync::Arc::as_ptr(&raster) as usize;
    if cache.as_ref().is_some_and(|c| c.raster == ptr) {
        let bitmap = cache.as_ref()?.bitmap.clone();
        return Some((bitmap, raster.width, raster.height));
    }
    let bitmap = f.gfx.bitmap_bgra(raster.width, raster.height, &raster.pixels).ok()?;
    *cache = Some(Bitmap { raster: ptr, bitmap: bitmap.clone() });
    Some((bitmap, raster.width, raster.height))
}

#[derive(Clone, Copy, PartialEq)]
pub enum Fit {
    Cover,
    Contain,
}

/// Draws an image inside a rounded rectangle without distorting its aspect ratio.
pub fn draw(f: &Frame, bitmap: &ID2D1Bitmap1, rect: D2D_RECT_F, radius: f32, alpha: f32, fit: Fit) -> Result<()> {
    // SAFETY: the bitmap is a live D2D object owned by the caller.
    let size = unsafe { bitmap.GetSize() };
    let (dw, dh) = (rect.right - rect.left, rect.bottom - rect.top);
    if size.width <= 0.0 || size.height <= 0.0 || dw <= 0.0 || dh <= 0.0 {
        return Ok(());
    }
    let scale = match fit {
        Fit::Cover => (dw / size.width).max(dh / size.height),
        Fit::Contain => (dw / size.width).min(dh / size.height),
    };
    let src = match fit {
        Fit::Cover => {
            let (sw, sh) = (dw / scale, dh / scale);
            D2D_RECT_F { left: (size.width - sw) / 2.0, top: (size.height - sh) / 2.0, right: (size.width + sw) / 2.0, bottom: (size.height + sh) / 2.0 }
        }
        // Contain always shows the whole image; cropping would go outside the source on extreme ratios.
        Fit::Contain => D2D_RECT_F { left: 0.0, top: 0.0, right: size.width, bottom: size.height },
    };
    let (w, h) = (size.width * scale, size.height * scale);
    let dst =
        D2D_RECT_F { left: rect.left + (dw - w) / 2.0, top: rect.top + (dh - h) / 2.0, right: rect.left + (dw + w) / 2.0, bottom: rect.top + (dh + h) / 2.0 };
    // SAFETY: the rounded geometry and layer parameters live until PopLayer; the mask is then released.
    unsafe {
        let rr = f.gfx.d2d.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect, radiusX: radius, radiusY: radius })?;
        let params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: rect,
            geometricMask: std::mem::ManuallyDrop::new(Some(rr.cast()?)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: alpha,
            opacityBrush: std::mem::ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        f.dc.PushLayer(&params, None);
        f.dc.DrawBitmap(bitmap, Some(&dst), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, Some(&src), None);
        f.dc.PopLayer();
        drop(std::mem::ManuallyDrop::into_inner(params.geometricMask));
    }
    Ok(())
}

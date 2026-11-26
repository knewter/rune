//! A window's last frame, kept so a frame that changes a small part of it draws only that part.
//!
//! A drawable's contents are undefined once presented, so the kept frame is a texture of its
//! own, the canvas, copied to each drawable whole. While a frame's damage (`damage.rs`) is a
//! small area, the canvas has that area cleared (a cleared scratch texture copied over it, so
//! the clear is the one a whole frame's pass makes) and drawn again under a scissor, with only
//! the isolated layers that can show there; every pixel it draws is computed as a whole frame
//! computes it. A frame damaged whole draws the whole canvas; after a few of those in a row it
//! draws straight into the drawable as before and lets the canvas go, so a scroll or a page's
//! own animation costs nothing extra. Nothing to adopt: a window gets it when its drawable can
//! be copied into, and anything the damage can't account for draws whole.
use super::damage::FrameDamage;
use super::*;

/// Frames damaged whole in a row after which the window draws into its drawable again.
const WHOLE_STREAK: u32 = 3;

/// The canvas a window keeps between frames, and what it holds.
#[derive(Default)]
pub(crate) struct Retained {
    canvas: Option<Canvas>,
    /// The renderer and prepare whose frame the canvas holds.
    holds: Option<(crate::compose::DeviceKey, u64)>,
    whole: u32,
}

struct Canvas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    scratch: Option<(wgpu::Texture, wgpu::TextureView)>,
    view_format: wgpu::TextureFormat,
    clear: [f32; 4],
}

/// How a frame reached its drawable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Drew {
    /// Into the drawable, as without a canvas.
    Direct,
    /// Whole into the canvas, then copied.
    Whole,
    /// Only `[x0, y0, x1, y1]` into the canvas, then copied.
    Area([u32; 4]),
    /// Nothing changed: the canvas copied.
    Kept,
}

impl Retained {
    /// Forgets the canvas: the next frame draws into its drawable.
    pub(crate) fn release(&mut self) {
        *self = Self::default();
    }
}

fn texture(device: &wgpu::Device, label: &str, format: wgpu::TextureFormat, view_format: wgpu::TextureFormat, (w, h): (u32, u32))
    -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: if view_format == format { &[] } else { std::slice::from_ref(&view_format) },
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(view_format), ..Default::default() });
    (texture, view)
}

fn copy(encoder: &mut wgpu::CommandEncoder, from: &wgpu::Texture, to: &wgpu::Texture, at: [u32; 2], (w, h): (u32, u32)) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo { texture: from, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyTextureInfo { texture: to, mip_level: 0, origin: wgpu::Origin3d { x: at[0], y: at[1], z: 0 }, aspect: wgpu::TextureAspect::All },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}

impl Renderer {
    /// Draws the prepared frame for `out`, a drawable (or a window's own intermediate) viewed as
    /// `view_format` through `out_view`, cleared to `clear`: through `retained`'s canvas when
    /// `out` can be copied into and the frame's damage allows, otherwise into `out_view` as
    /// `draw` does. The renderer's format must be `view_format`.
    pub(crate) fn draw_retained(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, retained: &mut Retained,
        out: &wgpu::Texture, out_view: &wgpu::TextureView, view_format: wgpu::TextureFormat, clear: [f32; 4]) -> Drew {
        let size = (out.width(), out.height());
        let [vw, vh] = self.target_frame(None).1;
        let plain = self.springs.is_none() && self.surfaces.as_ref().is_none_or(|s| s.is_empty())
            && out.usage().contains(wgpu::TextureUsages::COPY_DST) && view_format == self.format && size == (vw, vh);
        let mut direct = |retained: &mut Retained| {
            retained.release();
            self.draw(encoder, out_view, clear);
            Drew::Direct
        };
        if !plain { return direct(retained); }
        let current = retained.holds == Some((self.device_key, self.damage.prepares - 1))
            && retained.canvas.as_ref().is_some_and(|c| (c.texture.width(), c.texture.height()) == size
                && c.texture.format() == out.format() && c.view_format == view_format && c.clear == clear);
        let mut damage = self.damage.frame;
        if !current {
            // The canvas doesn't hold the frame before: take one only for a frame that changed
            // a small part of that one, drawing it whole.
            if damage == FrameDamage::Full { return direct(retained); }
            let (texture, view) = texture(device, "kept frame", out.format(), view_format, size);
            retained.canvas = Some(Canvas { texture, view, scratch: None, view_format, clear });
            retained.whole = 0;
            damage = FrameDamage::Full;
        } else if damage == FrameDamage::Full {
            retained.whole += 1;
            if retained.whole > WHOLE_STREAK { return direct(retained); }
        } else {
            retained.whole = 0;
        }
        let canvas = retained.canvas.as_mut().unwrap();
        retained.holds = Some((self.device_key, self.damage.prepares));
        let drew = match damage {
            FrameDamage::Full => {
                self.draw(encoder, &canvas.view, clear);
                Drew::Whole
            }
            FrameDamage::Nothing => {
                self.damage.counts.set(Default::default());
                Drew::Kept
            }
            FrameDamage::Area(area) => {
                self.draw_area(device, encoder, canvas, area, clear);
                Drew::Area(area)
            }
        };
        copy(encoder, &canvas.texture, out, [0, 0], size);
        drew
    }

    /// Clears `area` of the canvas as a whole frame's pass does and draws it again.
    fn draw_area(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, canvas: &mut Canvas, area: [u32; 4], clear: [f32; 4]) {
        let (w, h) = (area[2] - area[0], area[3] - area[1]);
        if canvas.scratch.as_ref().is_none_or(|(t, _)| t.width() < w || t.height() < h) {
            let bucket = (w.div_ceil(64) * 64, h.div_ceil(64) * 64);
            canvas.scratch = Some(texture(device, "kept frame clear", canvas.texture.format(), canvas.view_format, bucket));
        }
        let (scratch, scratch_view) = canvas.scratch.as_ref().unwrap();
        let color = wgpu::Color { r: clear[0] as f64, g: clear[1] as f64, b: clear[2] as f64, a: clear[3] as f64 };
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("kept frame clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: scratch_view, depth_slice: None, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(color), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
        }));
        copy(encoder, scratch, &canvas.texture, [area[0], area[1]], (w, h));
        self.damage.counts.set(Default::default());
        self.damage.add(|c| { c.cleared += u64::from(scratch.width()) * u64::from(scratch.height()); c.scissor = Some(area); });
        let within = [area[0] as f32, area[1] as f32, area[2] as f32, area[3] as f32];
        self.encode_layers_within(encoder, None, Some(within), &mut |_, _| {});
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("kept frame"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &canvas.view, depth_slice: None, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
        }).forget_lifetime();
        let mut active = ActivePass::owned(pass, encoder, &canvas.view).bounded([area[0], area[1], w, h]);
        self.draw_layer_range(&mut active, self.format, 0..self.count, None, &mut |_, _| {});
    }
}

#[cfg(test)]
#[path = "retained_tests.rs"]
mod tests;

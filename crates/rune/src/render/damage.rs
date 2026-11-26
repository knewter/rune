//! What a frame changed since the frame before it, for a window that keeps its last frame
//! (`retained.rs`) and redraws only that.
//!
//! `prepare` already writes only the instances that differ from what the instance buffer holds.
//! Each one that differs paints, before and after, inside the quad the vertex shader draws for it
//! (`base_vs` in `shader.wgsl`, mirrored by [`painted`]), or, inside an isolated layer, inside the
//! composite of the outermost layer with a target around it, which is all its change can reach.
//! Their union is the frame's damage. Whatever moves pixels without an instance changing (a glyph
//! or image upload, a new viewport or blend mode, effect slots and backdrop blurs, native
//! surfaces, GPU springs, a lost staging) damages the whole frame, and so does a change that
//! adds, removes or re-kinds an instance or covers more than half the window.
use super::*;
use std::cell::Cell;

/// Changed instances past this many damage the whole frame instead of being tracked.
const MAX_CHANGED: usize = 1024;

/// A damaged area past this share of the window is drawn whole.
const MAX_SHARE: f32 = 0.5;

/// What a prepared frame changed since the prepare before it, in device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FrameDamage {
    /// Draw all of it.
    Full,
    /// Nothing that paints changed.
    Nothing,
    /// Only pixels inside these edges `[x0, y0, x1, y1]` (whole pixels, inside the viewport).
    Area([u32; 4]),
}

/// What the last draw did, for tests and diagnostics: the main pass's scissor (`None`: the
/// whole target), the isolated-layer passes it encoded, and the pixels it cleared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameCounts {
    pub(crate) scissor: Option<[u32; 4]>,
    pub(crate) layer_passes: u32,
    pub(crate) cleared: u64,
}

/// A layer's footprint in one frame: its instance span, its planned bounds and, when it drew
/// into a target, the window edges its composite covers.
#[derive(Clone, Copy, Debug)]
pub(super) struct Foot {
    begin: u32,
    end: u32,
    bounds: [f32; 4],
    composite: Option<[f32; 4]>,
}

pub(super) struct Damage {
    /// The frame changed in a way instances don't say.
    full: bool,
    /// Set outside `prepare` (an image upload): the next prepared frame is damaged whole.
    pending_full: bool,
    /// Changed instances, with the union of their painted edges before and after.
    changed: Vec<(u32, Option<[f32; 4]>)>,
    /// The previous frame's layers, by plan index.
    feet: Vec<Foot>,
    pub(super) frame: FrameDamage,
    /// Prepares so far. A retained frame is only damaged relative to the prepare before it.
    pub(super) prepares: u64,
    pub(super) counts: Cell<FrameCounts>,
}

impl Default for Damage {
    fn default() -> Self {
        Self { full: true, pending_full: true, changed: Vec::new(), feet: Vec::new(), frame: FrameDamage::Full,
            prepares: 0, counts: Cell::new(FrameCounts::default()) }
    }
}

impl Damage {
    pub(super) fn begin(&mut self) {
        self.full = std::mem::take(&mut self.pending_full);
        self.changed.clear();
        self.prepares += 1;
    }

    /// Everything this frame shows may have changed.
    pub(super) fn all(&mut self) { self.full = true; }

    /// The next prepared frame may differ everywhere (textures written outside `prepare`).
    pub(super) fn all_next(&mut self) { self.pending_full = true; }

    /// Instance `at` was `old` and is now `new`.
    pub(super) fn changed(&mut self, at: usize, old: &Instance, new: &Instance) {
        if self.full { return; }
        let kind = new.params[1];
        if old.params[1] != kind || kind == KIND_EFFECT || kind == KIND_BACKDROP_BLUR || self.changed.len() >= MAX_CHANGED {
            self.full = true;
            return;
        }
        let edges = match (painted(old), painted(new)) {
            (Some(mut a), Some(b)) => { union(&mut a, b); Some(a) }
            (a, b) => a.or(b),
        };
        self.changed.push((at as u32, edges));
    }

    pub(super) fn add(&self, counts: impl FnOnce(&mut FrameCounts)) {
        let mut c = self.counts.get();
        counts(&mut c);
        self.counts.set(c);
    }

    /// Turns what `prepare` noted into the frame's damage, given this frame's layer footprints
    /// (`now`, which become the next frame's previous ones) and the viewport.
    pub(super) fn settle(&mut self, now: Vec<Foot>, viewport: (u32, u32)) {
        let before = std::mem::replace(&mut self.feet, now);
        self.frame = if self.full || before.len() != self.feet.len() {
            FrameDamage::Full
        } else if self.changed.is_empty() {
            FrameDamage::Nothing
        } else {
            let mut edges = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
            for &(at, painted) in &self.changed {
                if let Some(p) = painted { union(&mut edges, p); }
                for feet in [&before, &self.feet] {
                    // Feet are in plan order, outer layers first: the first one around `at` with
                    // a target is the composite its change reaches the window through.
                    if let Some(c) = feet.iter().filter(|f| f.begin <= at && at <= f.end).find_map(|f| f.composite) {
                        union(&mut edges, c);
                    }
                    // A changed scope record moves its clip or flattening: all it holds.
                    for f in feet.iter().filter(|f| f.begin == at || f.end == at) { union(&mut edges, f.bounds); }
                }
            }
            area(edges, viewport)
        };
    }
}

/// Whole pixels covering `edges` inside the viewport, or the whole frame when that's most of it.
fn area(edges: [f32; 4], (vw, vh): (u32, u32)) -> FrameDamage {
    if !edges.iter().all(|e| e.is_finite()) { return if edges[2] < edges[0] { FrameDamage::Nothing } else { FrameDamage::Full } }
    let x0 = edges[0].floor().clamp(0.0, vw as f32) as u32;
    let y0 = edges[1].floor().clamp(0.0, vh as f32) as u32;
    let x1 = edges[2].ceil().clamp(0.0, vw as f32) as u32;
    let y1 = edges[3].ceil().clamp(0.0, vh as f32) as u32;
    if x1 <= x0 || y1 <= y0 { return FrameDamage::Nothing; }
    let share = ((x1 - x0) as f32 * (y1 - y0) as f32) / (vw.max(1) as f32 * vh.max(1) as f32);
    if share > MAX_SHARE { FrameDamage::Full } else { FrameDamage::Area([x0, y0, x1, y1]) }
}

fn union(a: &mut [f32; 4], b: [f32; 4]) {
    a[0] = a[0].min(b[0]); a[1] = a[1].min(b[1]);
    a[2] = a[2].max(b[2]); a[3] = a[3].max(b[3]);
}

/// The window edges an instance's quad can cover, as `base_vs` places it, a pixel wider for
/// rasterization. `None` for what the main pipeline doesn't draw: scope records, effect slots.
pub(super) fn painted(i: &Instance) -> Option<[f32; 4]> {
    let kind = i.params[1];
    if kind == KIND_LAYER || kind == KIND_LAYER_END || kind > 11.5 { return None; }
    let grow = if kind > 0.5 && kind < 1.5 {
        i.params[2] * 3.0 + 1.0
    } else if kind > 1.5 && kind < 2.5 {
        0.0
    } else if kind > 2.5 && kind < 3.5 || kind > 4.5 && kind < 9.5 {
        1.0
    } else {
        1.0 + i.params[2] * 2.0
    };
    let r = i.rect;
    let (mut lo, mut hi) = ([r[0] - grow, r[1] - grow], [r[0] + r[2] + grow, r[1] + r[3] + grow]);
    // One band of a big shadow or border ring (runtime/bands.rs) rasterizes only its window.
    if kind < 1.5 && i.uv[2] < 0.0 {
        lo = [lo[0].max(r[0] + i.uv[0]), lo[1].max(r[1] + i.uv[1])];
        hi = [hi[0].min(r[0] - i.uv[2]).max(lo[0]), hi[1].min(r[1] + i.uv[3]).max(lo[1])];
    }
    let mut edges = [lo[0], lo[1], hi[0], hi[1]];
    // A looping icon turns its quad about its centre.
    if kind > 1.5 && kind < 2.5 && i.params[0] != 0.0 {
        edges = layers::turned_bounds(edges, i.params[0], [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5]);
    }
    if !edges.iter().all(|e| e.is_finite()) { return Some([f32::NEG_INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::INFINITY]); }
    Some([edges[0] - 1.0, edges[1] - 1.0, edges[2] + 1.0, edges[3] + 1.0])
}

impl Renderer {
    /// This frame's layer footprints (`Damage::settle`).
    pub(super) fn layer_feet(&self) -> Vec<Foot> {
        (0..self.layers_planned()).map(|index| {
            let (begin, end, bounds, composite) = self.layer_reach(index);
            Foot { begin, end, bounds, composite }
        }).collect()
    }

    /// What the last draw encoded: its main scissor, layer passes and cleared pixels.
    #[cfg(test)]
    pub(crate) fn frame_counts(&self) -> FrameCounts { self.damage.counts.get() }
}

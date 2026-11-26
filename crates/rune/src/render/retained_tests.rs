use super::*;
use crate::render::damage::FrameCounts;
use crate::{Color, Cx, El, Env, Icon, LoopSpec, Runtime, TextStyle, col, el, icon, row, text};

static TREFOIL: Icon = Icon {
    name: "kept-frame-trefoil",
    view: 24.0,
    body: r#"<path d="M12 3.5c2.2 0 3.4 2.1 2.6 4.2 2.2-.6 4.3.6 4.3 2.8 0 1.9-1.6 3.1-3.4 3.1 1.1 1.9.4 4.4-1.9 4.9-1.6.4-2.9-.6-3.6-1.9-.7 1.3-2 2.3-3.6 1.9-2.3-.5-3-3-1.9-4.9-1.8 0-3.4-1.2-3.4-3.1 0-2.2 2.1-3.4 4.3-2.8C8.6 5.6 9.8 3.5 12 3.5z" fill="currentColor"/>"#,
};

static OVAL: Icon = Icon { name: "kept-frame-oval", view: 24.0, body: r#"<ellipse cx="12" cy="12" rx="8.6" ry="5.4" fill="currentColor"/>"# };

const W: u32 = 320;
const H: u32 = 200;
/// Not black, and showing between the rows: an area's clear must be the frame's.
const CLEAR: [f32; 4] = [0.05, 0.1, 0.2, 1.0];

#[derive(Default)]
struct Page { pressed: u32, looping: bool, tint: bool }

/// A sidebar row carrying a running task's mark (a breathing group of two turning icons), over
/// rows of text, a card with a shadow, a translucent strip and blurred words, and a button.
fn view(s: &Page, _: &mut Cx<Page>) -> El {
    let ink = Color { r: 0.86, g: 0.87, b: 0.9, a: 1.0 };
    let pebble = el().key("pebble").size(16.0, 16.0).absolute(0.0, 0.0);
    let (trefoil, oval) = (icon(&TREFOIL, 16.0).absolute(0.0, 0.0).color(ink), icon(&OVAL, 16.0).absolute(0.0, 0.0).color(ink).icon_rotation(0.87));
    let pebble = if s.looping {
        pebble.looping(LoopSpec::opacity(2.4, 0.0, 0.5, 1.0))
            .child(trefoil.looping(LoopSpec::rotation(9.6))).child(oval.looping(LoopSpec::rotation(4.8)))
    } else {
        pebble.opacity(0.75).child(trefoil).child(oval)
    };
    let mut rows = col().gap(4.0).padding(8.0);
    for n in 0..5 {
        let mut r = row().h(22.0).gap(6.0).center_y().radius(4.0).fill(Color::WHITE.alpha(0.06));
        if n == 2 { r = r.child(el().key("mark").size(16.0, 16.0).child(pebble.clone())); }
        rows = rows.child(r.child(text(format!("Row {n}")).font(TextStyle::new(12.0, 400.0)).color(ink)));
    }
    let blurred = col().child(text("Over a picture").font(TextStyle::new(14.0, 600.0)).color(Color::BLACK).blur(2.0))
        .child(text("Over a picture").font(TextStyle::new(14.0, 600.0)).color(ink));
    let card = col().padding(10.0).radius(8.0).fill(Color { r: 0.15, g: 0.16, b: 0.19, a: 1.0 })
        .shadow(3.0, 8.0, Color::BLACK.alpha(0.5)).child(text(format!("Pressed {}", s.pressed)).color(ink));
    let button = el().key("button").size(60.0, 20.0).radius(10.0).fill(Color::WHITE.alpha(0.3))
        .on_press(|s: &mut Page| s.pressed += 1);
    let tint = el().key("tint").size(20.0, 20.0).radius(10.0).fill(Color::WHITE.alpha(0.5))
        .on_press(|s: &mut Page| s.tint = !s.tint);
    let page = row().w_full().h_full().gap(12.0).child(rows.w(140.0))
        .child(col().gap(12.0).padding(12.0).child(blurred).child(card).child(button).child(tint));
    // The root leaves the window's clear showing, until it's tinted all over.
    if s.tint { page.fill(Color { r: 0.3, g: 0.2, b: 0.1, a: 1.0 }) } else { page }
}

/// A window's drawable: copied into, and read back through an `Offscreen`.
struct Drawable { texture: wgpu::Texture, view: wgpu::TextureView, read: Offscreen }

impl Drawable {
    fn new(gpu: &Gpu, (w, h): (u32, u32)) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test drawable"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self { texture, view, read: Offscreen::new(&gpu.device, w, h) }
    }
}

/// Two windows over one runtime: `kept` draws through a canvas, `whole` draws every frame
/// whole into its target, as before. `kept` also draws each frame whole into `again`, after
/// its kept frame: what a whole redraw of the same prepared frame shows.
struct Pair { gpu: Gpu, kept: Renderer, retained: Retained, drawable: Drawable, whole: Renderer, target: Offscreen, again: Offscreen }

impl Pair {
    fn new(size: (u32, u32)) -> Self {
        let gpu = Gpu::headless();
        let (kept, whole) = (Renderer::new(&gpu.device, OFFSCREEN_FORMAT), Renderer::new(&gpu.device, OFFSCREEN_FORMAT));
        let drawable = Drawable::new(&gpu, size);
        let target = Offscreen::new(&gpu.device, size.0, size.1);
        let again = Offscreen::new(&gpu.device, size.0, size.1);
        Self { gpu, kept, retained: Retained::default(), drawable, whole, target, again }
    }

    fn resize(&mut self, size: (u32, u32)) {
        self.drawable = Drawable::new(&self.gpu, size);
        self.target = Offscreen::new(&self.gpu.device, size.0, size.1);
        self.again = Offscreen::new(&self.gpu.device, size.0, size.1);
    }

    /// Draws `rt`'s frame: what the canvas did, its counts, the kept frame and the same
    /// renderer's whole redraw of it, and the other window's whole frame.
    fn draw(&mut self, rt: &mut Runtime<Page>) -> (Drew, FrameCounts, Vec<u8>, Vec<u8>, Vec<u8>) {
        let (gpu, size) = (&self.gpu, (self.target.width, self.target.height));
        self.kept.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
        self.kept.prepare(&gpu.device, &gpu.queue, &mut rt.text.atlas, &rt.instances, size);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        let drew = self.kept.draw_retained(&gpu.device, &mut enc, &mut self.retained, &self.drawable.texture, &self.drawable.view, OFFSCREEN_FORMAT, CLEAR);
        let counts = self.kept.frame_counts();
        let read = &self.drawable.read;
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.drawable.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &read.buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(read.padded_row()), rows_per_image: Some(size.1) } },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 });
        self.kept.submit(&gpu.queue, enc);
        let kept = self.drawable.read.read(&gpu.device);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        self.kept.draw(&mut enc, &self.again.view, CLEAR);
        self.again.copy(&mut enc);
        self.kept.submit(&gpu.queue, enc);
        let again = self.again.read(&gpu.device);
        // The runtime's atlas hands each glyph row over once, to `kept`: hand it all to `whole`.
        rt.text.atlas.dirty = Some((0, crate::text::ATLAS_SIZE));
        for page in &mut rt.text.atlas.pages { page.dirty = Some((0, crate::text::ATLAS_SIZE)); }
        self.whole.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
        self.whole.prepare(&gpu.device, &gpu.queue, &mut rt.text.atlas, &rt.instances, size);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        self.whole.draw(&mut enc, &self.target.view, CLEAR);
        self.target.copy(&mut enc);
        self.whole.submit(&gpu.queue, enc);
        (drew, counts, kept, again, self.target.read(&gpu.device))
    }
}

fn runtime(looping: bool) -> Runtime<Page> {
    let mut rt = Runtime::new(Page { looping, ..Page::default() }, view, (W as f32, H as f32), Env::new(true, 1.0));
    rt.frame(0.0);
    rt
}

fn press(rt: &mut Runtime<Page>, key: &str) {
    let (x, y) = rt.find(key).unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
}

/// The kept frame is, bit for bit, the same renderer's whole redraw of the frame and the
/// other window's, which drew every frame whole.
fn identical(kept: &[u8], again: &[u8], whole: &[u8], frame: usize) {
    exact(kept, again, frame);
    exact(kept, whole, frame);
}

fn exact(kept: &[u8], whole: &[u8], frame: usize) {
    if let Some(at) = kept.chunks_exact(4).zip(whole.chunks_exact(4)).position(|(a, b)| a != b) {
        let (x, y) = (at as u32 % W, at as u32 / W);
        panic!("frame {frame}: the kept frame differs from a whole one at ({x}, {y}): {:?} != {:?}",
            &kept[at * 4..at * 4 + 4], &whole[at * 4..at * 4 + 4]);
    }
}

#[test]
fn a_loop_alone_redraws_only_its_mark_and_matches_a_whole_frame() {
    let mut rt = runtime(true);
    let mut pair = Pair::new((W, H));
    let (mark, _) = rt.find("mark").unwrap();
    for frame in 0..40 {
        let (drew, counts, kept, again, whole) = pair.draw(&mut rt);
        identical(&kept, &again, &whole, frame);
        // A whole frame clears the window and renders every isolated layer, the blurred words'
        // included: what each frame of the loop cost before.
        let all = pair.whole.frame_counts();
        assert!(all.cleared >= u64::from(W * H) && all.layer_passes >= 2 && all.scissor.is_none(), "frame {frame}: {all:?}");
        // The first frame draws into the drawable; the second, a small change, takes a canvas.
        if frame >= 2 {
            let Drew::Area(area) = drew else { panic!("frame {frame}: only the mark moved, but it drew {drew:?}") };
            let (w, h) = (area[2] - area[0], area[3] - area[1]);
            assert!(w <= 32 && h <= 32 && (area[0] as f32) <= mark && mark <= area[2] as f32, "frame {frame}: {area:?} around the mark at x {mark}");
            assert_eq!(counts.scissor, Some(area), "frame {frame}: the pass draws only the damaged area");
            assert_eq!(counts.layer_passes, 1, "frame {frame}: only the mark's own layer renders, not the blurred words");
            assert!(counts.cleared <= 2 * 64 * 64, "frame {frame}: cleared {} pixels", counts.cleared);
        }
        rt.frame(1.0 / 60.0);
    }
}

#[test]
fn a_change_during_the_loop_draws_a_correct_frame_and_the_loop_goes_on() {
    let mut rt = runtime(true);
    let mut pair = Pair::new((W, H));
    for frame in 0..4 { let (_, _, kept, again, whole) = pair.draw(&mut rt); identical(&kept, &again, &whole, frame); rt.frame(1.0 / 60.0); }
    // A press changes the card's words, away from the mark: both are drawn, and match.
    press(&mut rt, "button");
    let mut grew = false;
    for frame in 4..16 {
        rt.frame(1.0 / 60.0);
        let (drew, _, kept, again, whole) = pair.draw(&mut rt);
        identical(&kept, &again, &whole, frame);
        grew |= matches!(drew, Drew::Whole) || matches!(drew, Drew::Area(a) if a[2] - a[0] > 32);
    }
    assert_eq!(rt.state.pressed, 1);
    assert!(grew, "the press's change was drawn beyond the mark");
    // Most of the window changing (a fill under all of it) draws it whole; then the loop is
    // small again.
    press(&mut rt, "tint");
    rt.frame(1.0 / 60.0);
    let (drew, _, kept, again, whole) = pair.draw(&mut rt);
    identical(&kept, &again, &whole, 16);
    assert_eq!(drew, Drew::Whole, "a fill under everything repaints everything");
    // The fill fades in on its spring, every frame whole; once it lands, only the mark moves.
    let mut drew = Drew::Whole;
    for frame in 17..77 {
        rt.frame(1.0 / 60.0);
        let (now, _, kept, again, whole) = pair.draw(&mut rt);
        identical(&kept, &again, &whole, frame);
        drew = now;
    }
    assert!(matches!(drew, Drew::Area(_)), "{drew:?}");
    // A new size lets the canvas go and draws into the drawable; the loop then takes a new one.
    pair.resize((W, H - 20));
    rt.set_viewport((W as f32, (H - 20) as f32), 1.0);
    let mut kinds = Vec::new();
    for frame in 77..81 {
        rt.frame(1.0 / 60.0);
        let (drew, _, kept, again, whole) = pair.draw(&mut rt);
        identical(&kept, &again, &whole, frame);
        kinds.push(drew);
    }
    assert!(matches!(kinds[..], [Drew::Direct, Drew::Whole, Drew::Area(_), Drew::Area(_)]), "after a resize: {kinds:?}");
}

#[test]
fn a_frame_that_changes_nothing_copies_the_kept_one_and_whole_frames_let_it_go() {
    let mut rt = runtime(false);
    let mut pair = Pair::new((W, H));
    let (drew, _, kept, again, whole) = pair.draw(&mut rt);
    assert_eq!(drew, Drew::Direct, "a first frame needs no canvas");
    identical(&kept, &again, &whole, 0);
    // A press: a small change, which takes the canvas.
    press(&mut rt, "button");
    let mut kinds = Vec::new();
    for frame in 1..30 {
        rt.frame(1.0 / 60.0);
        let (drew, _, kept, again, whole) = pair.draw(&mut rt);
        identical(&kept, &again, &whole, frame);
        kinds.push(drew);
    }
    assert!(kinds.contains(&Drew::Whole) && kinds.last() == Some(&Drew::Kept), "{kinds:?}");
    // Frames damaged whole, one after another, draw straight into the drawable again.
    for frame in 30..36 {
        press(&mut rt, "tint");
        rt.frame(1.0 / 60.0);
        let (drew, _, kept, again, whole) = pair.draw(&mut rt);
        identical(&kept, &again, &whole, frame);
        if frame >= 30 + WHOLE_STREAK as usize { assert_eq!(drew, Drew::Direct, "frame {frame}"); }
    }
}

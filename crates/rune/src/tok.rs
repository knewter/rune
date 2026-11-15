//! Design tokens: the one place for colour, space, radius, type and motion.

use crate::color::{Color, oklch, oklcha};

pub mod motion {
    pub use rune_motion::motion::*;
    /// The leading edge of something travelling (width leads).
    pub const LEAD: rune_motion::Spring = rune_motion::Spring::new(0.2, 0.0);
}

pub mod space {
    pub const S1: f32 = 4.0;
    pub const S2: f32 = 8.0;
    pub const S3: f32 = 12.0;
    pub const S6: f32 = 24.0;
}

pub mod radius {
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 12.0;
}

/// An app font slot (`AppSpec::families`) or a family registered through `AppSpec::fonts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Family {
    #[default]
    Sans,
    Mono,
    /// A literal font family name, not a CSS list. Falls back to the sans slot if missing.
    Named(&'static str),
    /// The platform's monospaced UI font, loaded from the system on first use, never bundled:
    /// SF Mono on macOS (AppKit's `monospacedSystemFont`, Menlo before it existed), Cascadia
    /// Mono then Consolas on Windows, and fontconfig's `monospace` on Linux. Falls back to the
    /// mono slot when none is installed (headless on a bare machine, the web).
    SystemMono,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum OpticalSize {
    /// Use the run's point size, independent of display scale. Explicit `opsz` variations win.
    #[default]
    Auto,
    /// Leave optical sizing to Parley/the font (the legacy Rune behavior).
    FontDefault,
    /// Override `opsz`, in points, including an `opsz` in the variation settings. Also picks the
    /// `trak` tracking: text is tracked as Core Text tracks text of this size scaled to the set
    /// size.
    Fixed(f32),
}

/// Hashes as `==` compares, `-0.0` as `0.0`, so a style can be a `memo` version.
impl std::hash::Hash for OpticalSize {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        if let OpticalSize::Fixed(size) = self { crate::color::hash_bits(*size).hash(state) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: f32,
    pub family: Family,
    /// Line height in points; 0 means 1.25 × size.
    pub line_height: f32,
    /// Letter spacing in points, added to the font's own size-specific tracking (see `features`).
    pub tracking: f32,
    /// CSS OpenType settings, e.g. `"'opsz' 14, 'wght' 500"`. Empty preserves font defaults.
    pub variations: &'static str,
    /// CSS OpenType settings, e.g. `"'tnum' 1, 'ss01' 1"`. Fonts with a `trak` table (the macOS
    /// system font among them) are tracked at their point size (or explicit optical size), as Core
    /// Text does; `'trak' 0` turns that off.
    pub features: &'static str,
    pub optical_size: OpticalSize,
    /// The family's italic face, or a synthetic oblique when it has none.
    pub italic: bool,
}

/// Hashes as `==` compares, `-0.0` as `0.0`, so a style can be a `memo` version.
impl std::hash::Hash for TextStyle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let TextStyle { size, weight, family, line_height, tracking, variations, features, optical_size, italic } = *self;
        [size, weight, line_height, tracking].map(crate::color::hash_bits).hash(state);
        (family, variations, features, optical_size, italic).hash(state);
    }
}

impl TextStyle {
    pub const fn new(size: f32, weight: f32) -> TextStyle {
        TextStyle { size, weight, family: Family::Sans, line_height: 0.0, tracking: 0.0,
            variations: "", features: "", optical_size: OpticalSize::Auto, italic: false }
    }
    pub const fn italic(self) -> TextStyle {
        TextStyle { italic: true, ..self }
    }
    pub const fn mono(self) -> TextStyle {
        TextStyle { family: Family::Mono, ..self }
    }
    pub const fn family(self, family: Family) -> TextStyle {
        TextStyle { family, ..self }
    }
    pub const fn lh(self, line_height: f32) -> TextStyle {
        TextStyle { line_height, ..self }
    }
    pub const fn tracking(self, tracking: f32) -> TextStyle {
        TextStyle { tracking, ..self }
    }
    pub const fn variations(self, variations: &'static str) -> TextStyle {
        TextStyle { variations, ..self }
    }
    pub const fn features(self, features: &'static str) -> TextStyle {
        TextStyle { features, ..self }
    }
    /// Fixes `opsz` at `size` points and picks the `trak` tracking with it: text is tracked as
    /// Core Text tracks `size`-point text scaled to the set size, so
    /// `TextStyle::new(size * k, w).optical_size(size)` matches a SwiftUI view at `size` with
    /// `scaleEffect(k)`. Auto optical size tracks at the point size. To change only the outlines
    /// and keep the set size's tracking, as CoreText does for a font given an `opsz` variation,
    /// use `.variations("'opsz' 17")`: on SF at 16 pt, `.optical_size(17.0)` is 0.094 pt a glyph
    /// tighter (17-pt tracking scaled to 16), and both match CoreText's widths exactly.
    pub const fn optical_size(self, size: f32) -> TextStyle {
        TextStyle { optical_size: OpticalSize::Fixed(size), ..self }
    }
    pub const fn optical_sizing(self, optical_size: OpticalSize) -> TextStyle {
        TextStyle { optical_size, ..self }
    }
    /// Resolved line height in points.
    pub fn line_px(&self) -> f32 {
        if self.line_height > 0.0 { self.line_height } else { self.size * 1.25 }
    }
}

pub mod type_ {
    use super::TextStyle;
    pub const BODY: TextStyle = TextStyle::new(13.0, 400.0);
    pub const LABEL: TextStyle = TextStyle::new(12.0, 500.0);
    pub const SECTION: TextStyle = TextStyle::new(11.5, 600.0);
    pub const TITLE: TextStyle = TextStyle::new(13.0, 600.0);
    pub const MONO_CAPTION: TextStyle = TextStyle::new(12.0, 500.0);
}

/// Semantic colours for one appearance. Alphas are for linear-light blending, so white overlays
/// on dark are much lower and black overlays on light much higher than their sRGB/CSS equivalents.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub bg: Color,
    pub card: Color,
    pub card_border: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub separator: Color,
    pub control_track: Color,
    pub control_off: Color,
    pub indicator: Color,
    pub knob: Color,
    pub hover: Color,
    pub press: Color,
    pub shadow: Color,
    pub shadow_soft: Color,
    /// Hairline contact shadow under raised controls.
    pub edge: Color,
    pub knob_shadow: Color,
}

pub fn theme(dark: bool) -> Theme {
    if dark {
        Theme {
            bg: oklch(0.185, 0.004, 265.0),
            card: oklch(0.232, 0.005, 265.0),
            card_border: oklcha(1.0, 0.0, 0.0, 0.05),
            text: oklch(0.955, 0.003, 265.0),
            text_secondary: oklch(0.72, 0.008, 265.0),
            separator: oklcha(1.0, 0.0, 0.0, 0.055),
            control_track: oklcha(1.0, 0.0, 0.0, 0.05),
            control_off: oklcha(1.0, 0.0, 0.0, 0.085),
            indicator: oklch(0.43, 0.006, 265.0),
            knob: oklch(0.985, 0.0, 0.0),
            hover: oklcha(1.0, 0.0, 0.0, 0.016),
            press: oklcha(1.0, 0.0, 0.0, 0.03),
            shadow: oklcha(0.0, 0.0, 0.0, 0.42),
            shadow_soft: oklcha(0.0, 0.0, 0.0, 0.28),
            edge: oklcha(0.0, 0.0, 0.0, 0.3),
            knob_shadow: oklcha(0.0, 0.0, 0.0, 0.3),
        }
    } else {
        Theme {
            bg: oklch(0.958, 0.004, 265.0),
            card: oklch(1.0, 0.0, 0.0),
            card_border: oklcha(0.0, 0.0, 0.0, 0.1),
            text: oklch(0.225, 0.006, 265.0),
            text_secondary: oklch(0.5, 0.01, 265.0),
            separator: oklcha(0.0, 0.0, 0.0, 0.13),
            control_track: oklcha(0.2, 0.01, 265.0, 0.13),
            control_off: oklcha(0.2, 0.01, 265.0, 0.24),
            indicator: oklch(1.0, 0.0, 0.0),
            knob: oklch(1.0, 0.0, 0.0),
            hover: oklcha(0.2, 0.01, 265.0, 0.07),
            press: oklcha(0.2, 0.01, 265.0, 0.12),
            shadow: oklcha(0.2, 0.02, 265.0, 0.16),
            shadow_soft: oklcha(0.2, 0.02, 265.0, 0.07),
            edge: oklcha(0.2, 0.02, 265.0, 0.2),
            knob_shadow: oklcha(0.2, 0.02, 265.0, 0.3),
        }
    }
}

pub const ACCENT_NAMES: [&str; 6] = ["blue", "purple", "pink", "orange", "green", "graphite"];

pub fn accent(i: usize, dark: bool) -> Color {
    let l = if dark { 0.66 } else { 0.6 };
    match i % 6 {
        0 => oklch(l, 0.17, 256.0),
        1 => oklch(l, 0.18, 300.0),
        2 => oklch(l + 0.02, 0.19, 357.0),
        3 => oklch(l + 0.08, 0.16, 58.0),
        4 => oklch(l + 0.04, 0.16, 152.0),
        _ => oklch(l - 0.02, 0.012, 265.0),
    }
}

thread_local! {
    static TEXT_SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

/// Sets the factor every text style built on this thread is drawn at, from here on: an app
/// that zooms its whole window (a compact density) and wants text kept at the size the person
/// chose sets the inverse here. It applies where a style is given to an element ([`crate::text`],
/// [`crate::El::font`], a [`crate::text::Span`]'s size), so set it before the view builds;
/// a [`crate::memo`]'d subtree is built again when it changes. 1.0 (the default) changes nothing.
pub fn set_text_scale(scale: f32) {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    TEXT_SCALE.set(scale);
}

/// The factor [`set_text_scale`] set on this thread.
pub fn text_scale() -> f32 {
    TEXT_SCALE.get()
}

/// `style` at the thread's [`text_scale`]: its size, line height and tracking.
pub(crate) fn scaled(style: TextStyle) -> TextStyle {
    let k = text_scale();
    if k == 1.0 {
        return style;
    }
    TextStyle { size: style.size * k, line_height: style.line_height * k, tracking: style.tracking * k, ..style }
}

#[cfg(test)]
mod text_scale_tests {
    use super::*;

    #[test]
    fn text_built_under_a_scale_keeps_its_size_through_a_zoom() {
        set_text_scale(1.0 / 0.86);
        let body = crate::text("body");
        let fonted = crate::text("t").font(type_::BODY);
        let spans = crate::rich("ab", vec![crate::text::Span { start: 0, end: 1, size: Some(10.0), ..Default::default() }]);
        set_text_scale(1.0);
        let k = 1.0 / 0.86;
        assert!((body.text.as_ref().unwrap().style.size - type_::BODY.size * k).abs() < 1e-4);
        assert!((fonted.text.as_ref().unwrap().style.size - type_::BODY.size * k).abs() < 1e-4);
        assert!((spans.text.as_ref().unwrap().spans[0].size.unwrap() - 10.0 * k).abs() < 1e-4);
        assert_eq!(crate::text("x").text.as_ref().unwrap().style.size, type_::BODY.size, "back at 1.0");
    }
}

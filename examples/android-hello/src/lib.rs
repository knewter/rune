//! A Rune app on Android: a counter and the hello switch. Its state lives across the app going
//! to the background and coming back, when Android swaps the native window under it.
//! `scripts/android-apk.sh` builds and signs the APK; `cargo run -p android-hello` opens the
//! same view on the desktop.

use rune::tok::{self, motion, type_};
use rune::{AppSpec, Cx, El, col, el, text};

#[derive(Default)]
pub struct State {
    on: bool,
    taps: u32,
}

fn view(s: &State, _: &mut Cx<State>) -> El {
    let t = tok::theme(true);
    let label = text(if s.on { "Lights on" } else { "Lights off" }).font(type_::TITLE).color(t.text);
    let knob = el().key("knob").size(18.0, 18.0).radius(9.0).fill(t.knob)
        .absolute(if s.on { 18.0 } else { 2.0 }, 2.0);
    let switch = el().key("switch").role("switch").label("Lights").checked(s.on).focusable(true)
        .size(38.0, 22.0).radius(11.0)
        .fill(if s.on { tok::accent(0, true) } else { t.control_off })
        .motion(motion::SNAPPY)
        .on_press(|s: &mut State| s.on = !s.on)
        .child(knob);
    let count = text(format!("Tapped {} times", s.taps)).color(t.text);
    let button = el().key("tap").role("button").label("Tap").focusable(true)
        .size(160.0, 48.0).radius(12.0).fill(tok::accent(0, true)).center()
        .on_press(|s: &mut State| s.taps += 1)
        .child(text("Tap").color(t.bg));
    col().w_full().h_full().center().gap(16.0).fill(t.bg)
        .child(label).child(switch).child(count).child(button)
}

pub fn spec() -> AppSpec<State> {
    AppSpec {
        scenes: &[("on", |s| s.on = true), ("tapped", |s| s.taps = 3)],
        ..AppSpec::new("Android hello", (360.0, 640.0), State::default, view)
    }
}

/// NativeActivity's entry point (`android-activity`).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: rune::android::AndroidApp) {
    rune::android::init(app);
    rune::run(spec());
}

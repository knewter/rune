//! Rune on a `SurfaceView` inside an ordinary Kotlin `Activity`. The activity and its view are
//! Android's; Rune's runtime lives on the UI thread for the life of the process, so it outlives
//! the activity (Back, rotation, a relaunch into the cached process) and attaches to whatever
//! surface the current view has. Frames come from `Choreographer`; touches, the soft keyboard
//! (`InputConnection`), window insets and TalkBack (AccessKit) arrive over JNI (`android`).
//! `cargo run -p android-view` opens the same view on the desktop.

use rune::tok::{self, motion, type_};
use rune::{AppSpec, Cx, El, ScrollSpec, TextEdit, col, el, row, text, text_input};

#[cfg(target_os = "android")]
mod android;

#[derive(Default)]
pub struct State {
    on: bool,
    taps: u32,
    composer: TextEdit,
    sent: Vec<String>,
    picked: Option<usize>,
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = cx.system_dark();
    let t = tok::theme(dark);
    let accent = tok::accent(0, dark);
    let [top, right, bottom, left] = cx.safe_area();
    let label = text(if s.on { "Lights on" } else { "Lights off" }).font(type_::TITLE).color(t.text);
    let knob = el().key("knob").size(18.0, 18.0).radius(9.0).fill(t.knob)
        .absolute(if s.on { 18.0 } else { 2.0 }, 2.0);
    let switch = el().key("switch").role("switch").label("Lights").checked(s.on).focusable(true)
        .size(38.0, 22.0).radius(11.0)
        .fill(if s.on { accent } else { t.control_off })
        .motion(motion::SNAPPY)
        .on_press(|s: &mut State| s.on = !s.on)
        .child(knob);
    let button = el().key("tap").role("button").label("Tap").focusable(true)
        .size(140.0, 48.0).radius(12.0).fill(accent).center()
        .on_press(|s: &mut State| s.taps += 1)
        .child(text("Tap").color(t.bg));
    let count = text(format!("Tapped {} times", s.taps)).color(t.text);
    let info = text(format!("safe area {top:.0}/{right:.0}/{bottom:.0}/{left:.0} | scale {:.2} | picked {:?}",
        cx.env().scale, s.picked.map(|i| i + 1)))
        .font(type_::LABEL).color(t.text_secondary);

    let mut list = col().key("list").gap(6.0).w_full();
    for (i, m) in s.sent.iter().enumerate() {
        list = list.child(
            row().key(format!("sent-{i}")).h(44.0).px(16.0).center_y().radius(10.0).w_full().fill(accent.alpha(0.18))
                .child(text(m.clone()).color(t.text)),
        );
    }
    for i in 0..40 {
        let picked = s.picked == Some(i);
        list = list.child(
            row().key(format!("row-{i}")).role("button").label(format!("Row {}", i + 1)).focusable(true)
                .h(48.0).px(16.0).center_y().radius(10.0).w_full()
                .fill(if picked { accent.alpha(0.35) } else { t.card })
                .on_press(move |s: &mut State| s.picked = Some(i))
                .child(text(format!("Row {}", i + 1)).color(t.text)),
        );
    }

    // A composer at the bottom, as a chat has: the soft keyboard's inset lifts it.
    let composer = text_input(&s.composer, type_::BODY).key("composer").label("Message").single_line()
        .placeholder("Message", t.text_secondary).caret(t.text, accent.alpha(0.3))
        .color(t.text).grow(1.0).h(44.0).px(12.0).radius(10.0).fill(t.card)
        .on_edit(|s: &mut State| &mut s.composer)
        .on_submit(|s: &mut State, _| send(s));
    let send_button = el().key("send").role("button").label("Send").focusable(true)
        .size(72.0, 44.0).radius(10.0).fill(accent).center()
        .on_press(|s: &mut State| send(s))
        .child(text("Send").color(t.bg));

    col().w_full().h_full().fill(t.bg)
        .pt(top + 12.0).pr(right + 16.0).pb(bottom.max(12.0)).pl(left + 16.0).gap(12.0)
        .child(row().gap(16.0).center_y().child(label).child(switch))
        .child(row().gap(16.0).center_y().child(button).child(count))
        .child(info)
        .child(list.scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0))
        .child(row().gap(8.0).center_y().w_full().child(composer).child(send_button))
}

fn send(s: &mut State) {
    let message = s.composer.text.trim().to_owned();
    if message.is_empty() { return; }
    s.sent.push(message);
    s.composer.clear();
}

pub fn spec() -> AppSpec<State> {
    AppSpec::new("Rune on a SurfaceView", (360.0, 640.0), State::default, view)
}

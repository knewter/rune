//! A Rune app on Android, drawn straight into NativeActivity's window through wgpu: a counter,
//! a switch, a text field and a long list that scrolls under a finger. Its state lives across
//! the app going to the background and coming back, when Android swaps the native window under
//! it. `cargo run -p android-hello` opens the same view on the desktop.

use rune::tok::{self, motion, type_};
use rune::{AppSpec, Cx, El, ScrollSpec, TextEdit, col, el, row, text, text_input};

#[derive(Default)]
pub struct State {
    on: bool,
    taps: u32,
    field: TextEdit,
    picked: Option<usize>,
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = cx.system_dark();
    let t = tok::theme(dark);
    let env = cx.env();
    let label = text(if s.on { "Lights on" } else { "Lights off" }).font(type_::TITLE).color(t.text);
    let knob = el().key("knob").size(18.0, 18.0).radius(9.0).fill(t.knob)
        .absolute(if s.on { 18.0 } else { 2.0 }, 2.0);
    let switch = el().key("switch").role("switch").label("Lights").checked(s.on).focusable(true)
        .size(38.0, 22.0).radius(11.0)
        .fill(if s.on { tok::accent(0, dark) } else { t.control_off })
        .motion(motion::SNAPPY)
        .on_press(|s: &mut State| s.on = !s.on)
        .child(knob);
    let count = text(format!("Tapped {} times", s.taps)).color(t.text);
    let button = el().key("tap").role("button").label("Tap").focusable(true)
        .size(160.0, 48.0).radius(12.0).fill(tok::accent(0, dark)).center()
        .on_press(|s: &mut State| s.taps += 1)
        .child(text("Tap").color(t.bg));
    let field = text_input(&s.field, type_::BODY).key("field").single_line()
        .placeholder("Type here", t.text_secondary).caret(t.text, tok::accent(0, dark).alpha(0.3))
        .color(t.text).w_full().h(44.0).px(12.0).radius(10.0).fill(t.card)
        .on_edit(|s: &mut State| &mut s.field);
    let echo = text(format!("Field: \"{}\"", s.field.text)).font(type_::LABEL).color(t.text_secondary);
    let mut list = col().key("list").gap(6.0).w_full();
    for i in 0..60 {
        let picked = s.picked == Some(i);
        list = list.child(
            row().key(format!("row-{i}")).h(48.0).px(16.0).center_y().radius(10.0).w_full()
                .fill(if picked { tok::accent(0, dark).alpha(0.35) } else { t.card })
                .on_press(move |s: &mut State| s.picked = Some(i))
                .child(text(format!("Row {}", i + 1)).color(t.text)),
        );
    }
    let info = text(format!("scale {:.2} | dark {} | picked {:?}", env.scale, dark, s.picked.map(|i| i + 1)))
        .font(type_::LABEL).color(t.text_secondary);
    col().w_full().h_full().padding(20.0).gap(12.0).fill(t.bg)
        .child(row().gap(16.0).center_y().child(label).child(switch))
        .child(row().gap(16.0).center_y().child(button).child(count))
        .child(field).child(echo).child(info)
        .child(list.scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0))
}

pub fn spec() -> AppSpec<State> {
    AppSpec {
        scenes: &[("on", |s| s.on = true), ("tapped", |s| s.taps = 3)],
        ..AppSpec::new("Android hello", (360.0, 640.0), State::default, view)
    }
}

#[cfg(target_os = "android")]
mod logcat {
    //! Rune reports through stderr, which Android sends nowhere: copy fds 1 and 2 to logcat.
    use std::io::{BufRead, BufReader};
    use std::os::fd::FromRawFd;
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(prio: i32, tag: *const std::ffi::c_char, text: *const std::ffi::c_char) -> i32;
    }
    unsafe extern "C" {
        fn pipe(fds: *mut i32) -> i32;
        fn dup2(old: i32, new: i32) -> i32;
    }
    pub fn redirect() {
        let mut fds = [0i32; 2];
        // SAFETY: plain POSIX calls on fds this process owns.
        unsafe {
            if pipe(fds.as_mut_ptr()) != 0 { return; }
            dup2(fds[1], 1);
            dup2(fds[1], 2);
        }
        let read = unsafe { std::fs::File::from_raw_fd(fds[0]) };
        std::thread::spawn(move || {
            for line in BufReader::new(read).lines().map_while(Result::ok) {
                let line = std::ffi::CString::new(line).unwrap_or_default();
                unsafe { __android_log_write(4, c"RuneStderr".as_ptr(), line.as_ptr()) };
            }
        });
    }
}

/// NativeActivity's entry point (`android-activity`).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: rune::android::AndroidApp) {
    logcat::redirect();
    std::panic::set_hook(Box::new(|info| eprintln!("panic: {info}")));
    eprintln!("android_main: starting rune");
    rune::android::init(app);
    rune::run(spec());
    // winit makes one EventLoop per process, and Android keeps a finished activity's process
    // cached: a relaunch into it panicked "EventLoop can't be recreated". End the process
    // with the activity, so a relaunch starts fresh.
    eprintln!("android_main: rune::run returned; exiting the process");
    std::process::exit(0);
}

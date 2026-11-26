//! One small looping mark in a large, otherwise still window: a sidebar of rows, a page of text
//! and a few cards, with one row carrying a 16-point "working" mark that breathes (a looping
//! opacity on the group holding it) and turns (two looping icon rotations), the shape of an
//! app's "this task is running" mark. For measuring what a frame of that loop costs.
//!
//!   cargo run -p rune --example loop_mark --profile perf            # 1440x900, prints frames a second
//!   cargo run -p rune --example loop_mark --profile perf -- --still  # the same window at rest
//!
//! `--shadows` adds blurred copies under the sidebar's titles (isolated, blurred layers), as an
//! app that sets words over a picture does. Each second the window drew anything it prints
//! `loop_mark: <n> drawn/s` on standard error.
use rune::{App, AppSpec, Color, Cx, El, FrameOutcome, Icon, LoopSpec, TextStyle, col, el, icon, row, text};
use std::time::Instant;

static TREFOIL: Icon = Icon {
    name: "loop-mark-trefoil",
    view: 24.0,
    body: r#"<path d="M12 3.5c2.2 0 3.4 2.1 2.6 4.2 2.2-.6 4.3.6 4.3 2.8 0 1.9-1.6 3.1-3.4 3.1 1.1 1.9.4 4.4-1.9 4.9-1.6.4-2.9-.6-3.6-1.9-.7 1.3-2 2.3-3.6 1.9-2.3-.5-3-3-1.9-4.9-1.8 0-3.4-1.2-3.4-3.1 0-2.2 2.1-3.4 4.3-2.8C8.6 5.6 9.8 3.5 12 3.5z" fill="currentColor"/>"#,
};

static OVAL: Icon = Icon {
    name: "loop-mark-oval",
    view: 24.0,
    body: r#"<ellipse cx="12" cy="12" rx="8.6" ry="5.4" fill="currentColor"/>"#,
};

struct State {
    still: bool,
    shadows: bool,
}

fn mark(still: bool, ink: Color) -> El {
    let side = 16.0;
    let trefoil = icon(&TREFOIL, side).absolute(0.0, 0.0).color(ink);
    let oval = icon(&OVAL, side).absolute(0.0, 0.0).color(ink).icon_rotation(0.87);
    let pebble = el().key("pebble").size(side, side).absolute(0.0, 0.0);
    let pebble = if still {
        pebble.opacity(0.75).child(trefoil).child(oval)
    } else {
        pebble
            .looping(LoopSpec::opacity(2.4, 0.0, 0.5, 1.0))
            .child(trefoil.looping(LoopSpec::rotation(9.6)))
            .child(oval.looping(LoopSpec::rotation(4.8)))
    };
    el().key("mark").size(side, side).child(pebble)
}

fn title(words: &str, ink: Color, ground: Color, shadows: bool) -> El {
    let label = text(words.to_owned()).font(TextStyle::new(15.0, 600.0)).color(ink);
    if !shadows { return label; }
    let mut under = col().pin(Some(0.0), Some(1.0), Some(0.0), Some(-1.0));
    for _ in 0..3 {
        under = under.child(text(words.to_owned()).font(TextStyle::new(15.0, 600.0)).color(ground).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).blur(2.5));
    }
    col().child(under).child(label)
}

fn view(s: &State, _: &mut Cx<State>) -> El {
    let ground = Color { r: 0.11, g: 0.12, b: 0.14, a: 1.0 };
    let side = Color { r: 0.15, g: 0.16, b: 0.19, a: 1.0 };
    let ink = Color { r: 0.86, g: 0.87, b: 0.9, a: 1.0 };
    let muted = Color { r: 0.6, g: 0.62, b: 0.68, a: 1.0 };
    let mut bar = col().w(280.0).h_full().fill(side).padding(14.0).gap(6.0)
        .child(title("Projects", ink, ground, s.shadows));
    for n in 0..24 {
        let mut r = row().h(28.0).gap(8.0).px(8.0).radius(6.0).center_y();
        if n == 3 {
            r = r.fill(Color::WHITE.alpha(0.06)).child(mark(s.still, muted));
        }
        bar = bar.child(r.child(text(format!("A conversation about item {n}")).font(TextStyle::new(13.0, 400.0)).color(if n == 3 { ink } else { muted })));
    }
    let mut page = col().grow(1.0).h_full().padding(32.0).gap(14.0).child(title("A long page", ink, ground, s.shadows));
    for n in 0..8 {
        page = page.child(
            col().fill(side).radius(10.0).padding(16.0).gap(6.0).shadow(4.0, 12.0, Color::BLACK.alpha(0.4))
                .child(text(format!("Card {n}")).font(TextStyle::new(15.0, 600.0)).color(ink))
                .child(text("Some words that wrap across the card, as a message in a chat does, so the page has text all over it and every frame that redraws the window has to shade it again.".to_owned()).font(TextStyle::new(13.0, 400.0)).color(muted)),
        );
    }
    row().w_full().h_full().fill(ground).child(bar).child(page)
}

fn init() -> State {
    let flag = |name: &str| std::env::args().any(|a| a == name);
    State { still: flag("--still"), shadows: flag("--shadows") }
}

fn main() {
    let mut second = Instant::now();
    let mut drawn = 0u32;
    App::new(AppSpec::new("Loop mark", (1440.0, 900.0), init, view))
        .on_frame_encoded(move |frame| {
            if frame.outcome() == FrameOutcome::Drawn { drawn += 1; }
            if second.elapsed().as_secs_f64() >= 1.0 {
                if drawn > 0 { eprintln!("loop_mark: {drawn} drawn/s"); }
                drawn = 0;
                second = Instant::now();
            }
        })
        .run();
}

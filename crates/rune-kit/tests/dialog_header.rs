use rune::{Cx, El, Env, Runtime};
use rune_kit::{Dialog, Kit, root};

/// A phone's width, a finger's touch target, and a title longer than the line.
fn view(_: &(), cx: &mut Cx<()>) -> El {
    let mut k = Kit::neutral(cx.system_dark());
    k.touch_target = 48.0;
    let mut layer = Vec::new();
    Dialog::new("ask", "Someone on Telegram wants to talk to your agent from their own phone")
        .width(520.0)
        .open(true)
        .on_close(|_: &mut ()| {})
        .child(rune::el().size(100.0, 40.0))
        .build(&k, &mut layer);
    root(rune::el().w_full().h_full(), layer)
}

/// The close button stays inside the panel: a long title wraps beside it rather than pushing
/// it past the panel's edge, as it did on a phone (half the X outside the dialog).
#[test]
fn a_long_title_wraps_and_the_close_button_stays_inside_the_panel() {
    let mut rt = Runtime::new((), view, (412.0, 915.0), Env { system_dark: false, scale: 2.625 });
    for _ in 0..60 { rt.frame(1.0 / 60.0); }
    let elements = rt.elements();
    let rect = |key: &str| elements.iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no {key}")).rect;
    let (panel, close) = (rect("ask-dialog"), rect("close"));
    let right = |r: [f32; 4]| r[0] + r[2];
    assert!(close[0] >= panel[0] && right(close) <= right(panel) + 0.5,
        "the close button {close:?} is inside the panel {panel:?}");
    assert!(right(panel) <= 412.0 + 0.5, "the panel fits the phone: {panel:?}");
}

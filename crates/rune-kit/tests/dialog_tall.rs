use rune::{Cx, El, Env, Runtime};
use rune_kit::{Dialog, Kit, root};

/// A dialog whose body is `body` points tall, in a window `window` points high.
fn runtime(body: f32, window: f32) -> Runtime<f32> {
    fn view(body: &f32, cx: &mut Cx<f32>) -> El {
        let k = Kit::neutral(cx.system_dark());
        let mut layer = Vec::new();
        Dialog::new("pair", "Pair a device")
            .width(560.0)
            .fit()
            .open(true)
            .on_close(|_: &mut f32| {})
            .child(rune::el().w_full().h(*body))
            .build(&k, &mut layer);
        root(rune::el().w_full().h_full(), layer)
    }
    let mut rt = Runtime::new(body, view, (960.0, window), Env { system_dark: false, scale: 2.0 });
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    rt
}

fn rect(rt: &Runtime<f32>, key: &str) -> [f32; 4] {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no {key}")).rect
}

/// A dialog taller than a television's window opens at its top, its Close inside the
/// window: centred, the panel was cut off at both ends, and the focused Close above it.
#[test]
fn a_dialog_taller_than_the_window_opens_at_its_top() {
    let rt = runtime(900.0, 540.0);
    let (panel, close) = (rect(&rt, "pair-dialog"), rect(&rt, "close"));
    assert!(panel[1] >= 0.0 && panel[1] <= 16.5, "the panel starts at the window's top: {panel:?}");
    assert!(close[1] >= 0.0 && close[1] + close[3] <= 540.0, "Close is in the window: {close:?}");
    assert!(panel[3] > 540.0, "the panel keeps its height and scrolls: {panel:?}");
}

/// One that fits is centred, as it always was.
#[test]
fn a_dialog_that_fits_is_centred() {
    let rt = runtime(100.0, 540.0);
    let panel = rect(&rt, "pair-dialog");
    let middle = panel[1] + panel[3] / 2.0;
    assert!((middle - 270.0).abs() < 1.0, "centred: {panel:?}");
}

/// A remote walking down a tall dialog whose body scrolls on its own (a 560-point list inside
/// the layer that scrolls): each control it focuses is in the window. Revealing focus in the
/// list alone left five controls focused below the window on a television.
#[test]
fn walking_a_tall_dialog_keeps_focus_in_the_window() {
    fn view(_: &(), cx: &mut Cx<()>) -> El {
        let k = Kit::neutral(cx.system_dark());
        let rows = (0..12).map(|i| rune::el().key(format!("row-{i}")).focusable(true).w_full().h(70.0));
        let list = rune::col().w_full().children(rows).scroll(rune::ScrollSpec { stick_bottom: false }).max_h(560.0);
        let mut layer = Vec::new();
        Dialog::new("pair", "Pair a device")
            .width(560.0)
            .fit()
            .open(true)
            .on_close(|_: &mut ()| {})
            .child(rune::col().w_full().child(list).child(rune::el().key("create").focusable(true).w_full().h(48.0)))
            .build(&k, &mut layer);
        root(rune::el().w_full().h_full(), layer)
    }
    let mut rt = Runtime::new((), view, (960.0, 540.0), Env { system_dark: false, scale: 2.0 });
    rt.set_spatial_arrows(true);
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
    let mut walked = Vec::new();
    for _ in 0..16 {
        rt.key(rune::KeyEvent { key: rune::Key::Down, mods: Default::default() });
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        let Some(focused) = rt.elements().into_iter().find(|e| e.focused) else { continue };
        let [_, y, _, h] = focused.rect;
        walked.push((focused.key.clone(), y));
        assert!(y >= -0.5 && y + h <= 540.5, "{:?} focused at y {y} (h {h}) after {walked:?}", focused.key);
    }
    assert!(walked.iter().any(|(key, _)| key.as_deref() == Some("create")), "the walk reaches the end: {walked:?}");
}

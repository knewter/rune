use super::*;
use crate::{Color, ScrollSpec, col, el};

#[derive(Default)]
struct S {
    taps: Vec<usize>,
}

/// A button with nothing above it that takes gestures, over a list of rows that scrolls; each
/// lights when hovered and when pressed.
fn view(_: &S, _: &mut Cx<S>) -> El {
    col().size(300.0, 400.0)
        .child(el().key("free").w_full().h(40.0).hover(Color::WHITE).press(Color::WHITE).on_press(|s: &mut S| s.taps.push(99)))
        .child(col().key("list").w_full().h(300.0).scroll(ScrollSpec { stick_bottom: false })
            .children((0..30).map(|i| el().key(format!("row{i}")).w_full().h(40.0).hover(Color::WHITE).press(Color::WHITE)
                .on_press(move |s: &mut S| s.taps.push(i)))))
}

fn rig() -> Runtime<S> {
    let mut rt = Runtime::new(S::default(), view, (300.0, 400.0), Env::default());
    rt.frame(0.0);
    rt
}

fn id(rt: &Runtime<S>, key: &str) -> NodeId {
    rt.nodes.iter().find(|n| n.key.as_deref() == Some(key)).unwrap().id
}

/// The press track's target and value, and the hover track's target, for `key`.
fn looks(rt: &Runtime<S>, key: &str) -> (f32, f32, f32) {
    let id = id(rt, key);
    let store = rt.scene.store();
    (store.target(id, P_PRESS).unwrap_or(0.0), rt.scene.get(id, P_PRESS).unwrap_or(0.0), store.target(id, P_HOVER).unwrap_or(0.0))
}

/// The most any element shows pressed or hovered now.
fn lit_anywhere(rt: &Runtime<S>) -> f32 {
    rt.nodes.iter().map(|n| {
        let store = rt.scene.store();
        [store.target(n.id, P_PRESS), rt.scene.get(n.id, P_PRESS), store.target(n.id, P_HOVER), rt.scene.get(n.id, P_HOVER)]
            .into_iter().flatten().fold(0.0f32, f32::max)
    }).fold(0.0, f32::max)
}

#[test]
fn a_touch_that_pans_within_the_tap_timeout_never_shows_a_press_or_a_hover() {
    let mut rt = rig();
    let (x, y) = rt.find("row3").unwrap();
    let mut seen = 0.0f32;
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(0.02);
    seen = seen.max(lit_anywhere(&rt));
    for k in 1..=5 {
        rt.touch(1, TouchPhase::Moved, x, y - 12.0 * k as f32, 0.02 + 0.01 * k as f64);
        rt.frame(0.01);
        seen = seen.max(lit_anywhere(&rt));
    }
    // Rests past the timeout mid-pan: still nothing.
    for _ in 0..10 {
        rt.frame(0.02);
        seen = seen.max(lit_anywhere(&rt));
    }
    rt.touch(1, TouchPhase::Ended, x, y - 60.0, 0.4);
    for _ in 0..10 {
        rt.frame(0.02);
        seen = seen.max(lit_anywhere(&rt));
    }
    assert_eq!(seen, 0.0, "no frame showed a row pressed or hovered");
    assert!(rt.state.taps.is_empty(), "and nothing was pressed: {:?}", rt.state.taps);
}

#[test]
fn a_finger_resting_past_the_tap_timeout_shows_the_press_and_a_pan_lets_it_go_unpressed() {
    let mut rt = rig();
    let (x, y) = rt.find("row3").unwrap();
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(0.05);
    assert_eq!(looks(&rt, "row3").0, 0.0, "not before the tap timeout");
    rt.frame(0.06);
    let (target, value, hover) = looks(&rt, "row3");
    assert!(target == 1.0 && value > 0.0 && hover == 0.0, "pressed after it, and never hovered: {:?}", (target, value, hover));
    rt.touch(1, TouchPhase::Moved, x, y - 40.0, 0.15);
    rt.frame(0.02);
    assert_eq!(looks(&rt, "row3").0, 0.0, "the pan takes the press back");
    rt.touch(1, TouchPhase::Ended, x, y - 40.0, 0.2);
    rt.frame(0.02);
    assert!(rt.state.taps.is_empty(), "unpressed: {:?}", rt.state.taps);
}

#[test]
fn a_quick_tap_presses_once_and_flashes_its_press() {
    let mut rt = rig();
    let (x, y) = rt.find("row3").unwrap();
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(0.02);
    assert_eq!(looks(&rt, "row3").1, 0.0, "held back");
    rt.touch(1, TouchPhase::Ended, x, y, 0.04);
    rt.frame(1.0 / 120.0);
    assert_eq!(rt.state.taps, [3]);
    let (target, value, hover) = looks(&rt, "row3");
    assert!(target == 0.0 && value > 0.3, "the press flashes as it lets go: {value}");
    assert_eq!(hover, 0.0);
    for _ in 0..30 { rt.frame(1.0 / 60.0); }
    assert_eq!(looks(&rt, "row3").1, 0.0, "and fades");
    assert_eq!(rt.state.taps, [3], "once");
}

#[test]
fn where_nothing_above_takes_the_gesture_the_press_shows_at_once() {
    let mut rt = rig();
    let (x, y) = rt.find("free").unwrap();
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(1.0 / 120.0);
    let (target, value, hover) = looks(&rt, "free");
    assert!(target == 1.0 && value > 0.0 && hover == 0.0, "pressed, not hovered: {:?}", (target, value, hover));
    rt.touch(1, TouchPhase::Ended, x, y, 0.03);
    rt.frame(1.0 / 120.0);
    assert_eq!(rt.state.taps, [99]);
}

#[test]
fn a_mouse_hovers_and_presses_at_once_as_before() {
    let mut rt = rig();
    let (x, y) = rt.find("row3").unwrap();
    rt.pointer_move(x, y);
    rt.frame(1.0 / 120.0);
    assert_eq!(looks(&rt, "row3").2, 1.0, "hovered");
    rt.pointer_down(x, y);
    rt.frame(1.0 / 120.0);
    let (target, value, hover) = looks(&rt, "row3");
    assert!(target == 1.0 && value > 0.0 && hover == 1.0, "pressed at once inside the list: {:?}", (target, value, hover));
    rt.pointer_up(x, y);
    rt.frame(1.0 / 120.0);
    assert_eq!(rt.state.taps, [3]);
}

#[test]
fn the_host_sets_the_tap_timeout_and_the_slop() {
    let mut rt = rig();
    rt.set_tap_timeout(0.3);
    rt.set_touch_slop(20.0);
    assert_eq!((rt.tap_timeout(), rt.touch_slop()), (0.3, 20.0));
    let (x, y) = rt.find("row3").unwrap();
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(0.15);
    assert_eq!(looks(&rt, "row3").0, 0.0, "still held back at 0.15 s");
    // 15 points is inside a 20-point slop: still a tap.
    rt.touch(1, TouchPhase::Moved, x, y - 15.0, 0.2);
    rt.frame(0.2);
    assert_eq!(looks(&rt, "row3").0, 1.0, "shown after 0.3 s");
    rt.touch(1, TouchPhase::Ended, x, y - 15.0, 0.4);
    rt.frame(0.02);
    assert_eq!(rt.state.taps, [3]);
}

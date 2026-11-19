use super::super::TouchPhase;
use crate::{Cx, El, Env, Runtime, col, el, text};

#[derive(Default)]
struct S {
    taps: usize,
}

/// A line of selectable text in a pressable row, and room below it.
fn view(_: &S, _: &mut Cx<S>) -> El {
    col().size(400.0, 300.0).child(
        el().key("row").w_full().on_press(|s: &mut S| s.taps += 1)
            .child(text("alpha beta gamma").key("line").selectable(true)),
    )
}

fn rig() -> Runtime<S> {
    let mut rt = Runtime::new(S::default(), view, (400.0, 300.0), Env::default());
    rt.frame(0.0);
    rt
}

fn line(rt: &Runtime<S>) -> [f32; 4] {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some("line")).expect("the line").rect
}

/// Frames for `secs` of held time.
fn hold(rt: &mut Runtime<S>, secs: f64) {
    for _ in 0..(secs / 0.05).round() as usize { rt.frame(0.05); }
}

#[test]
fn a_finger_held_on_text_selects_the_word_under_it_and_presses_nothing() {
    let mut rt = rig();
    let r = line(&rt);
    let (x, y) = (r[0] + 4.0, r[1] + r[3] / 2.0);
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    hold(&mut rt, 0.3);
    assert_eq!(rt.selected_text().filter(|t| !t.is_empty()), None, "not yet: a press still waits");
    hold(&mut rt, 0.3);
    assert_eq!(rt.selected_text().as_deref(), Some("alpha"), "the word under the finger");
    rt.touch(1, TouchPhase::Ended, x, y, 0.6);
    rt.frame(0.05);
    assert_eq!(rt.state.taps, 0, "the row under it was not pressed");
    assert_eq!(rt.selected_text().as_deref(), Some("alpha"), "lifting keeps it");
    assert!(rt.selection_handles().is_some(), "and it has handles to move");
}

#[test]
fn moving_the_finger_after_the_long_press_extends_the_selection_by_words() {
    let mut rt = rig();
    let r = line(&rt);
    let y = r[1] + r[3] / 2.0;
    rt.touch(1, TouchPhase::Started, r[0] + 4.0, y, 0.0);
    hold(&mut rt, 0.6);
    rt.touch(1, TouchPhase::Moved, r[0] + r[2] - 2.0, y, 0.7);
    rt.touch(1, TouchPhase::Ended, r[0] + r[2] - 2.0, y, 0.8);
    rt.frame(0.05);
    assert_eq!(rt.selected_text().as_deref(), Some("alpha beta gamma"));
    assert_eq!(rt.state.taps, 0);
}

#[test]
fn a_tap_presses_and_selects_nothing() {
    let mut rt = rig();
    let r = line(&rt);
    let (x, y) = (r[0] + 4.0, r[1] + r[3] / 2.0);
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(0.05);
    rt.touch(1, TouchPhase::Ended, x, y, 0.05);
    hold(&mut rt, 0.8);
    assert_eq!(rt.state.taps, 1);
    assert_eq!(rt.selected_text().filter(|t| !t.is_empty()), None);
    assert!(rt.selection_handles().is_none());
}

#[test]
fn a_handle_moves_its_end_and_leaves_the_other() {
    let mut rt = rig();
    let r = line(&rt);
    let y = r[1] + r[3] / 2.0;
    rt.touch(1, TouchPhase::Started, r[0] + 4.0, y, 0.0);
    hold(&mut rt, 0.6);
    rt.touch(1, TouchPhase::Ended, r[0] + 4.0, y, 0.6);
    rt.frame(0.05);
    let [_, (ex, ey)] = rt.selection_handles().expect("handles");
    // The finger lands on the end handle, below the line, and carries it to the line's end.
    rt.touch(2, TouchPhase::Started, ex + 2.0, ey + 10.0, 1.0);
    rt.touch(2, TouchPhase::Moved, r[0] + r[2] - 1.0, ey + 10.0, 1.1);
    rt.frame(0.05);
    rt.touch(2, TouchPhase::Ended, r[0] + r[2] - 1.0, ey + 10.0, 1.2);
    rt.frame(0.05);
    assert_eq!(rt.selected_text().as_deref(), Some("alpha beta gamma"), "the end moved, the start stayed");
    assert_eq!(rt.state.taps, 0, "taking a handle presses nothing");
    // The start handle, carried right past the first word, leaves the rest.
    let [(sx, sy), _] = rt.selection_handles().expect("handles");
    let past = rt.selection_handles().map(|[s, e]| s.0 + (e.0 - s.0) * 0.4).unwrap();
    rt.touch(3, TouchPhase::Started, sx, sy + 10.0, 2.0);
    rt.touch(3, TouchPhase::Moved, past, sy + 10.0, 2.1);
    rt.touch(3, TouchPhase::Ended, past, sy + 10.0, 2.2);
    rt.frame(0.05);
    let left = rt.selected_text().unwrap_or_default();
    assert!(left.ends_with("gamma") && !left.starts_with("alpha"), "the start moved, the end stayed: {left:?}");
}

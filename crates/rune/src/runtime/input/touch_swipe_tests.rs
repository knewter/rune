//! A finger's swipe (`El::on_swipe`): sideways it is the swipe of the element around what it
//! pressed; up or down it still pans, a sideways viewport with room still scrolls, a long
//! press still selects, and an element that drags still drags.
use super::*;
use crate::{ScrollSpec, col, el, row, text};

#[derive(Default)]
struct S {
    taps: Vec<usize>,
    /// Each swipe phase heard: (phase, dx, velocity, cancelled).
    swipes: Vec<(Phase, f32, f32, bool)>,
    drags: usize,
    /// Each `on_swipe_touch` heard.
    touches: Vec<bool>,
}

/// A page that swipes, holding a vertical list of pressable rows, a sideways strip with room
/// to scroll, a knob that drags itself and a line of selectable text.
fn view(_: &S, _: &mut Cx<S>) -> El {
    col().key("page").size(300.0, 500.0)
        .on_swipe(|s: &mut S, d: &mut Drag, _| s.swipes.push((d.phase, d.dx, d.velocity, d.cancelled())))
        .child(col().key("list").w_full().h(200.0).scroll(ScrollSpec { stick_bottom: false })
            .children((0..30).map(|i| el().key(format!("row{i}")).w_full().h(40.0).on_press(move |s: &mut S| s.taps.push(i)))))
        .child(row().key("strip").w_full().h(60.0).scroll_x()
            .children((0..10).map(|i| el().key(format!("tile{i}")).size(100.0, 60.0))))
        .child(el().key("knob").w_full().h(40.0).on_drag(|s: &mut S, _: &mut Drag, _| s.drags += 1))
        .child(text("alpha beta gamma delta").key("line").selectable(true))
        .child(el().key("rest").w_full().h(160.0))
}

fn rig() -> Runtime<S> {
    let mut rt = Runtime::new(S::default(), view, (300.0, 500.0), Env::default());
    rt.frame(0.0);
    rt
}

fn offset(rt: &Runtime<S>, key: &str, horizontal: bool) -> f64 {
    let id = rt.nodes.iter().find(|n| n.key.as_deref() == Some(key)).unwrap().id;
    let st = &rt.scrolls[&id];
    if horizontal { st.x.offset } else { st.offset }
}

fn rect(rt: &Runtime<S>, key: &str) -> [f32; 4] {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).expect(key).rect
}

/// A finger from (x, y) moving by (dx, dy) over `steps` events 10 ms apart, then lifted.
fn swipe(rt: &mut Runtime<S>, (x, y): (f32, f32), (dx, dy): (f32, f32), steps: usize) {
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    for i in 1..=steps {
        let k = i as f32 / steps as f32;
        rt.touch(1, TouchPhase::Moved, x + dx * k, y + dy * k, i as f64 * 0.01);
    }
    rt.touch(1, TouchPhase::Ended, x + dx, y + dy, steps as f64 * 0.01 + 0.005);
    rt.frame(1.0 / 60.0);
}

#[test]
fn a_sideways_finger_on_a_row_is_the_pages_swipe_and_presses_nothing() {
    let mut rt = rig();
    swipe(&mut rt, (20.0, 100.0), (120.0, 6.0), 6);
    let phases: Vec<_> = rt.state.swipes.iter().map(|s| s.0).collect();
    assert_eq!(phases.first(), Some(&Phase::Down));
    assert_eq!(phases.last(), Some(&Phase::Up));
    assert!(phases.contains(&Phase::Move));
    let (_, dx, velocity, cancelled) = *rt.state.swipes.last().unwrap();
    assert_eq!(dx, 120.0, "the lift is where the finger went");
    assert!(velocity > 1000.0, "120 points in 60 ms is a fling: {velocity}");
    assert!(!cancelled);
    assert!(rt.state.taps.is_empty(), "the row it landed on was not pressed");
    assert_eq!(offset(&rt, "list", false), 0.0, "and the list did not move");
}

#[test]
fn a_finger_that_rests_before_lifting_throws_nothing() {
    let mut rt = rig();
    rt.touch(1, TouchPhase::Started, 20.0, 100.0, 0.0);
    rt.touch(1, TouchPhase::Moved, 80.0, 100.0, 0.01);
    rt.touch(1, TouchPhase::Ended, 80.0, 100.0, 0.5);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.swipes.last().map(|s| (s.0, s.2)), Some((Phase::Up, 0.0)));
}

#[test]
fn a_vertical_finger_still_scrolls_the_list_and_swipes_nothing() {
    let mut rt = rig();
    swipe(&mut rt, (150.0, 180.0), (10.0, -100.0), 5);
    assert!(rt.state.swipes.is_empty(), "{:?}", rt.state.swipes);
    assert!(offset(&rt, "list", false) >= 90.0);
}

#[test]
fn a_sideways_viewport_with_room_still_takes_a_sideways_finger() {
    let mut rt = rig();
    let r = rect(&rt, "strip");
    swipe(&mut rt, (250.0, r[1] + 30.0), (-120.0, 0.0), 6);
    assert!(rt.state.swipes.is_empty(), "{:?}", rt.state.swipes);
    assert!(offset(&rt, "strip", true) >= 100.0, "the strip scrolled: {}", offset(&rt, "strip", true));
}

#[test]
fn an_element_that_drags_still_drags() {
    let mut rt = rig();
    let r = rect(&rt, "knob");
    swipe(&mut rt, (20.0, r[1] + 20.0), (120.0, 0.0), 6);
    assert!(rt.state.swipes.is_empty());
    assert!(rt.state.drags > 2);
}

#[test]
fn a_long_press_still_selects_and_a_sideways_move_extends_it() {
    let mut rt = rig();
    let r = rect(&rt, "line");
    let y = r[1] + r[3] / 2.0;
    rt.touch(1, TouchPhase::Started, r[0] + 4.0, y, 0.0);
    for _ in 0..12 { rt.frame(0.05); }
    rt.touch(1, TouchPhase::Moved, r[0] + r[2] - 2.0, y, 0.7);
    rt.touch(1, TouchPhase::Ended, r[0] + r[2] - 2.0, y, 0.8);
    rt.frame(0.05);
    assert_eq!(rt.selected_text().as_deref(), Some("alpha beta gamma delta"));
    assert!(rt.state.swipes.is_empty());
}

#[test]
fn a_second_finger_cancels_the_swipe() {
    let mut rt = rig();
    rt.touch(1, TouchPhase::Started, 20.0, 100.0, 0.0);
    rt.touch(1, TouchPhase::Moved, 80.0, 100.0, 0.01);
    rt.touch(2, TouchPhase::Started, 150.0, 300.0, 0.02);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.swipes.last().map(|s| (s.0, s.3)), Some((Phase::Up, true)));
}

/// A page that swipes and hears a finger land (`El::on_swipe_touch`), as a gallery whose
/// picture is still settling stops it under the finger.
fn touched_view(_: &S, _: &mut Cx<S>) -> El {
    col().key("page").size(300.0, 500.0)
        .on_swipe(|s: &mut S, d: &mut Drag, _| s.swipes.push((d.phase, d.dx, d.velocity, d.cancelled())))
        .on_swipe_touch(|s: &mut S, down: bool, _| s.touches.push(down))
        .child(el().key("row").w_full().h(40.0).on_press(|s: &mut S| s.taps.push(0)))
        .child(el().key("knob").w_full().h(40.0).on_drag(|s: &mut S, _: &mut Drag, _| s.drags += 1))
        .child(el().key("rest").w_full().h(420.0))
}

#[test]
fn a_finger_landing_in_a_swipe_is_heard_before_it_moves() {
    let mut rt = Runtime::new(S::default(), touched_view, (300.0, 500.0), Env::default());
    rt.frame(0.0);
    let (x, y) = rt.find("rest").unwrap();
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.touches, [true], "heard at the touch, with no move");
    assert!(rt.state.swipes.is_empty(), "not a swipe yet");
    // Then it swipes as before, and the touch is not heard again.
    rt.touch(1, TouchPhase::Moved, x - 40.0, y, 0.05);
    rt.touch(1, TouchPhase::Ended, x - 40.0, y, 0.1);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.touches, [true], "a finger that swipes lifts as the swipe's Up");
    assert_eq!(rt.state.swipes.first().map(|s| s.0), Some(Phase::Down));
    // A finger on a row inside it is heard landing and lifting, and its tap still presses the row.
    let (x, y) = rt.find("row").unwrap();
    rt.touch(2, TouchPhase::Started, x, y, 1.0);
    rt.touch(2, TouchPhase::Ended, x, y, 1.05);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.touches, [true, true, false]);
    assert_eq!(rt.state.taps, [0]);
    // What drags itself takes the finger: the swipe hears nothing of it.
    let (x, y) = rt.find("knob").unwrap();
    rt.touch(3, TouchPhase::Started, x, y, 2.0);
    rt.touch(3, TouchPhase::Ended, x, y, 2.05);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.touches, [true, true, false], "a knob that drags is not the swipe's");
}

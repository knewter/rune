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

#[derive(Default)]
struct Page {
    taps: usize,
    menus: Vec<&'static str>,
}

/// Forty lines of selectable text in a scroll viewport 200 points tall, each on a row with a
/// margin a finger can land on beside the words, over a bar the viewport stops above.
fn page(_: &Page, _: &mut Cx<Page>) -> El {
    col().size(400.0, 300.0)
        .child(col().key("list").w_full().h(200.0).scroll(crate::ScrollSpec { stick_bottom: false })
            .children((0..40).map(|i| {
                el().key(format!("row{i}")).w_full().h(30.0).pl(100.0)
                    .on_press(|s: &mut Page| s.taps += 1)
                    .on_secondary_press(|s: &mut Page, _, _| s.menus.push("row"))
                    .child(text(format!("line{i} beta gamma")).key(format!("words{i}")).selectable(true))
            })))
        .child(el().key("bar").w_full().h(100.0).on_press(|s: &mut Page| s.taps += 100))
}

/// A window on a host's clock, as an embedder's is, last drawn `idle` seconds ago.
fn hosted(idle: f64) -> Runtime<Page> {
    let mut rt = Runtime::new(Page::default(), page, (400.0, 300.0), Env::default());
    rt.wall_start = Some(crate::time::Instant::now());
    rt.frame(1.0 / 60.0);
    pass(&mut rt, idle);
    rt
}

/// `secs` of host time go by with no frame: the clock's start moves back by as much.
fn pass(rt: &mut Runtime<Page>, secs: f64) {
    rt.wall_start = rt.wall_start.and_then(|t| t.checked_sub(std::time::Duration::from_secs_f64(secs)));
    assert!(rt.wall_start.is_some());
}

fn scrolled(rt: &Runtime<Page>) -> f64 {
    let id = rt.nodes.iter().find(|n| n.key.as_deref() == Some("list")).unwrap().id;
    rt.scrolls[&id].offset
}

fn rect(rt: &Runtime<Page>, key: &str) -> [f32; 4] {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("{key} is drawn")).rect
}

/// A finger down on the words of line 3, a frame, then dragged up `dy` points over `secs`, a
/// frame after each of its ten moves, and lifted.
fn drag(rt: &mut Runtime<Page>, dy: f32, secs: f64) {
    let r = rect(rt, "words3");
    let (x, y) = (r[0] + 4.0, r[1] + r[3] / 2.0);
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(1.0 / 60.0);
    for i in 1..=10 {
        pass(rt, secs / 10.0);
        rt.touch(1, TouchPhase::Moved, x, y + dy * i as f32 / 10.0, secs * i as f64 / 10.0);
        rt.frame(1.0 / 60.0);
    }
    rt.touch(1, TouchPhase::Ended, x, y + dy, secs + 0.2);
    rt.frame(1.0 / 60.0);
}

#[test]
fn a_drag_over_text_in_a_window_that_sat_idle_scrolls_and_selects_nothing() {
    for secs in [0.1, 0.2, 0.3, 0.6, 1.2] {
        let mut rt = hosted(30.0);
        drag(&mut rt, -60.0, secs);
        assert_eq!(rt.selected_text().filter(|t| !t.is_empty()), None, "a {secs} s drag selected");
        assert!(rt.selection_handles().is_none(), "a {secs} s drag left handles");
        assert_eq!(scrolled(&rt), 60.0, "a {secs} s drag scrolled");
        assert_eq!(rt.state.taps, 0);
    }
}

#[test]
fn the_long_press_is_timed_from_the_touch_not_from_the_last_frame() {
    let mut rt = hosted(30.0);
    let r = rect(&rt, "words3");
    let (x, y) = (r[0] + 4.0, r[1] + r[3] / 2.0);
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(1.0 / 60.0);
    pass(&mut rt, 0.3);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.selected_text().filter(|t| !t.is_empty()), None, "0.3 s is a press still");
    pass(&mut rt, 0.3);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.selected_text().as_deref(), Some("line3"), "0.6 s held still selects the word");
    // Moving now extends it, and scrolls nothing.
    let end = rect(&rt, "words3");
    rt.touch(1, TouchPhase::Moved, end[0] + end[2] - 2.0, y, 0.7);
    rt.touch(1, TouchPhase::Ended, end[0] + end[2] - 2.0, y, 0.8);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.selected_text().as_deref(), Some("line3 beta gamma"));
    assert_eq!(scrolled(&rt), 0.0);
    assert_eq!(rt.state.taps, 0);
}

#[test]
fn the_host_sets_the_long_press_time() {
    let mut rt = hosted(30.0);
    assert_eq!(rt.long_press_timeout(), 0.5);
    rt.set_long_press_timeout(1.0);
    let r = rect(&rt, "words3");
    let (x, y) = (r[0] + 4.0, r[1] + r[3] / 2.0);
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(1.0 / 60.0);
    pass(&mut rt, 0.7);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.selected_text().filter(|t| !t.is_empty()), None, "a person asked for a second");
    pass(&mut rt, 0.4);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.selected_text().as_deref(), Some("line3"));
}

#[test]
fn a_finger_that_left_the_slop_and_came_back_never_selects() {
    let mut rt = hosted(30.0);
    let r = rect(&rt, "words3");
    let (x, y) = (r[0] + 4.0, r[1] + r[3] / 2.0);
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(1.0 / 60.0);
    rt.touch(1, TouchPhase::Moved, x, y - 20.0, 0.05);
    rt.touch(1, TouchPhase::Moved, x, y, 0.1);
    pass(&mut rt, 2.0);
    rt.frame(1.0 / 60.0);
    rt.touch(1, TouchPhase::Ended, x, y, 2.1);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.selected_text().filter(|t| !t.is_empty()), None);
    assert_eq!(rt.state.taps, 0, "it was a pan");
}

#[test]
fn a_long_press_off_the_words_opens_the_rows_menu_and_presses_nothing() {
    let mut rt = hosted(30.0);
    let r = rect(&rt, "row3");
    let (x, y) = (r[0] + 20.0, r[1] + r[3] / 2.0);
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(1.0 / 60.0);
    pass(&mut rt, 0.6);
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.menus, ["row"], "as a right-click there does");
    rt.touch(1, TouchPhase::Moved, x, y - 60.0, 0.7);
    rt.touch(1, TouchPhase::Ended, x, y - 60.0, 0.8);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.taps, 0, "the row was not pressed");
    assert_eq!(scrolled(&rt), 0.0, "and the finger that opened a menu pans nothing");
    assert_eq!(rt.selected_text().filter(|t| !t.is_empty()), None);
    assert!(!rt.touching());
}

#[test]
fn a_handle_scrolled_out_of_its_viewport_takes_no_finger() {
    let mut rt = hosted(30.0);
    // The list goes on some lines, a word of line 7 is selected, and the list comes back:
    // line 7 starts 210 points down, under the bar the viewport stops above.
    rt.wheel(200.0, 100.0, -90.0);
    for _ in 0..60 { rt.frame(1.0 / 60.0); }
    assert_eq!(scrolled(&rt), 90.0);
    let r = rect(&rt, "words7");
    let (x, y) = (r[0] + 4.0, r[1] + r[3] / 2.0);
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(1.0 / 60.0);
    pass(&mut rt, 0.6);
    rt.frame(1.0 / 60.0);
    rt.touch(1, TouchPhase::Ended, x, y, 0.6);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.selected_text().as_deref(), Some("line7"));
    let clip = rt.selection_clip().expect("the viewport it is read in");
    assert!((clip[3] - 200.0).abs() < 0.5, "the list ends above the bar: {clip:?}");
    rt.wheel(200.0, 100.0, 1000.0);
    for _ in 0..60 { rt.frame(1.0 / 60.0); }
    assert_eq!(scrolled(&rt), 0.0);
    let [_, (ex, ey)] = rt.selection_handles().expect("the selection is kept");
    assert!(ey > 200.0, "its end is under the bar now: {ey}");
    rt.touch(3, TouchPhase::Started, ex + 2.0, ey + 10.0, 2.0);
    rt.touch(3, TouchPhase::Ended, ex + 2.0, ey + 10.0, 2.05);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.taps, 100, "the bar was pressed, not the handle");
}

#[derive(Default)]
struct Seen {
    clip: std::cell::Cell<Option<[f32; 4]>>,
    path: std::cell::RefCell<Option<String>>,
}

/// A view that reads where the selection shows, as one drawing handles over it does.
fn reading(s: &Seen, cx: &mut Cx<Seen>) -> El {
    let boxes = cx.selection_rects();
    s.clip.set(cx.selection_clip().filter(|_| !boxes.is_empty()));
    *s.path.borrow_mut() = cx.selection_path();
    col().size(400.0, 300.0)
        .child(el().key("head").w_full().h(40.0))
        .child(col().key("list").w_full().h(160.0).scroll(crate::ScrollSpec { stick_bottom: false })
            .children((0..40).map(|i| text(format!("line{i} beta gamma")).key(format!("words{i}")).h(30.0).selectable(true))))
}

#[test]
fn a_view_reads_the_viewport_the_selection_shows_in_and_which_text_it_is() {
    let mut rt = Runtime::new(Seen::default(), reading, (400.0, 300.0), Env::default());
    rt.frame(0.0);
    assert_eq!(rt.state.clip.get(), None);
    assert_eq!(*rt.state.path.borrow(), None);
    let r = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("words2")).unwrap().rect;
    rt.touch(1, TouchPhase::Started, r[0] + 4.0, r[1] + r[3] / 2.0, 0.0);
    for _ in 0..12 { rt.frame(0.05); }
    rt.touch(1, TouchPhase::Ended, r[0] + 4.0, r[1] + r[3] / 2.0, 0.6);
    for _ in 0..3 { rt.frame(0.05); }
    assert_eq!(rt.selected_text().as_deref(), Some("line2"));
    let clip = rt.state.clip.get().expect("the list's viewport");
    assert!((clip[1] - 40.0).abs() < 0.5 && (clip[3] - 200.0).abs() < 0.5, "from under the head to the list's foot: {clip:?}");
    assert!(rt.state.path.borrow().as_deref().is_some_and(|p| p.split('/').any(|k| k == "words2")), "{:?}", rt.state.path.borrow());
}

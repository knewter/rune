use super::super::TouchPhase;
use crate::{Cx, El, Env, Runtime, TextEdit, TextStyle, col, el, text_input};

struct Form {
    field: TextEdit,
    lines: TextEdit,
}

const WORDS: &str = "alpha brave charlie delta";

fn view(s: &Form, cx: &mut Cx<Form>) -> El {
    let style = TextStyle::new(14.0, 400.0).lh(20.0);
    // What a view reads of the selection, shown as an element a test can find.
    let in_field = cx.selection_in_field();
    let clipped = cx.selection_clip().is_some();
    col().size(400.0, 300.0).padding(8.0).gap(8.0)
        .child(text_input(&s.field, style).single_line().key("field").size(300.0, 24.0).on_edit(|s: &mut Form| &mut s.field))
        .child(text_input(&s.lines, style).key("lines").size(300.0, 80.0).on_edit(|s: &mut Form| &mut s.lines))
        .child(el().key(match (in_field, clipped) { (true, true) => "read-field", (false, true) => "read-text", _ => "read-none" }).size(1.0, 1.0))
}

/// What the view last read of the selection.
fn read(rt: &Runtime<Form>) -> String {
    rt.elements().into_iter().filter_map(|e| e.key).find(|k| k.starts_with("read-")).unwrap_or_default()
}

fn rig() -> Runtime<Form> {
    let state = Form { field: TextEdit::new(WORDS), lines: TextEdit::new("first line of words\nsecond line here") };
    let mut rt = Runtime::new(state, view, (400.0, 300.0), Env::default());
    rt.frame(0.0);
    rt
}

fn rect(rt: &Runtime<Form>, key: &str) -> [f32; 4] {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).expect("the field").rect
}

/// Frames for `secs` of held time.
fn hold(rt: &mut Runtime<Form>, secs: f64) {
    for _ in 0..(secs / 0.05).round() as usize { rt.frame(0.05); }
}

/// The x of byte `at` of the single-line field's text, by asking where a press lands.
fn x_of(rt: &Runtime<Form>, word: &str) -> f32 {
    let r = rect(rt, "field");
    let at = WORDS.find(word).unwrap() + word.len() / 2;
    // Fourteen-point text is about seven points a character; close enough to land inside a word.
    r[0] + 4.0 + at as f32 * 7.0
}

fn selected(edit: &TextEdit) -> &str {
    let (a, b) = edit.range();
    &edit.text[a..b]
}

/// A real finger's long press: it lands, wanders a few points inside the slop while it rests,
/// and lifts where it ended up (`adb shell input swipe x y x+3 y+2 700` on a phone).
fn jittering_press(rt: &mut Runtime<Form>, x: f32, y: f32) {
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    for (i, (dx, dy)) in [(1.0, 0.0), (2.0, 1.0), (1.0, 2.0), (3.0, 1.0), (2.0, 2.0), (3.0, 2.0)].into_iter().enumerate() {
        rt.frame(0.05);
        rt.touch(1, TouchPhase::Moved, x + dx, y + dy, 0.05 + 0.1 * i as f64);
        rt.frame(0.05);
    }
    hold(rt, 0.1);
    rt.touch(1, TouchPhase::Ended, x + 3.0, y + 2.0, 0.7);
    rt.frame(0.05);
}

#[test]
fn a_long_press_in_a_field_selects_the_word_under_the_finger() {
    let mut rt = rig();
    let r = rect(&rt, "field");
    let (x, y) = (x_of(&rt, "brave"), r[1] + r[3] / 2.0);
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    hold(&mut rt, 0.3);
    assert_eq!(selected(&rt.state.field), "", "not yet: a press still waits");
    hold(&mut rt, 0.3);
    assert!(rt.state.field.focused, "the field takes focus, as a tap gives it");
    assert_eq!(selected(&rt.state.field), "brave", "the word under the finger");
    rt.touch(1, TouchPhase::Ended, x, y, 0.6);
    rt.frame(0.05);
    assert_eq!(selected(&rt.state.field), "brave", "lifting keeps it");
}

#[test]
fn a_finger_that_wanders_inside_the_slop_still_selects() {
    let mut rt = rig();
    let r = rect(&rt, "field");
    let x = x_of(&rt, "charlie");
    jittering_press(&mut rt, x, r[1] + r[3] / 2.0);
    assert_eq!(selected(&rt.state.field), "charlie", "a few points of wander is still a long press");
}

#[test]
fn a_field_selection_has_a_handle_at_each_end_and_a_view_reads_its_boxes() {
    let mut rt = rig();
    let r = rect(&rt, "field");
    let x = x_of(&rt, "brave");
    jittering_press(&mut rt, x, r[1] + r[3] / 2.0);
    let [(sx, sy), (ex, ey)] = rt.selection_handles().expect("a field's selection has handles");
    assert!(sx < ex, "the start's left of the end: {sx} {ex}");
    assert!((sy - ey).abs() < 0.5 && sy > r[1] + 4.0 && sy <= r[1] + r[3] + 0.5, "both hang from the line's foot: {sy} {ey} in {r:?}");
    let boxes = rt.selection_boxes();
    assert_eq!(boxes.len(), 1, "one line box");
    let clip = rt.selection_clip().expect("the field's box is where its selection shows");
    assert!(clip[0] >= r[0] - 0.5 && clip[2] <= r[0] + r[2] + 0.5, "inside the field: {clip:?} {r:?}");
    assert!(rt.selection_in_field(), "and the runtime knows it is a field's");
    rt.frame(0.05);
    assert_eq!(read(&rt), "read-field", "a view reads the field's selection, and where it shows");
}

#[test]
fn a_tap_in_a_field_places_the_caret_and_selects_nothing() {
    let mut rt = rig();
    let r = rect(&rt, "field");
    let (x, y) = (x_of(&rt, "brave"), r[1] + r[3] / 2.0);
    rt.touch(1, TouchPhase::Started, x, y, 0.0);
    rt.frame(0.05);
    rt.touch(1, TouchPhase::Ended, x + 1.0, y, 0.08);
    hold(&mut rt, 0.8);
    assert!(rt.state.field.focused);
    assert_eq!(selected(&rt.state.field), "");
    assert!(rt.selection_handles().is_none(), "a caret has no selection handles");
}

#[test]
fn the_end_handle_of_a_field_moves_the_end_and_leaves_the_start() {
    let mut rt = rig();
    let r = rect(&rt, "field");
    let x = x_of(&rt, "brave");
    jittering_press(&mut rt, x, r[1] + r[3] / 2.0);
    let [_, (ex, ey)] = rt.selection_handles().expect("handles");
    // The finger lands on the drop below the line and carries it to the start of "delta".
    let to = x_of(&rt, "delta") - 10.0;
    rt.touch(2, TouchPhase::Started, ex + 2.0, ey + 12.0, 1.0);
    rt.touch(2, TouchPhase::Moved, (ex + to) / 2.0, ey + 13.0, 1.05);
    rt.frame(0.05);
    rt.touch(2, TouchPhase::Moved, to, ey + 12.0, 1.1);
    rt.frame(0.05);
    rt.touch(2, TouchPhase::Ended, to, ey + 12.0, 1.2);
    rt.frame(0.05);
    let s = selected(&rt.state.field).to_owned();
    assert!(s.starts_with("brave charlie"), "the end moved on, the start stayed: {s:?}");
    // And the start handle, carried left to the first word, leaves the end.
    let [(sx, sy), _] = rt.selection_handles().expect("handles");
    rt.touch(3, TouchPhase::Started, sx - 2.0, sy + 12.0, 2.0);
    rt.touch(3, TouchPhase::Moved, r[0] + 5.0, sy + 12.0, 2.1);
    rt.frame(0.05);
    rt.touch(3, TouchPhase::Ended, r[0] + 5.0, sy + 12.0, 2.2);
    rt.frame(0.05);
    let s = selected(&rt.state.field).to_owned();
    assert!(s.starts_with("alpha") || s.starts_with("lpha"), "the start moved: {s:?}");
    assert!(s.ends_with(&*selected_tail(&s)), "the end stayed");
    assert!(s.contains("charlie"), "the end stayed past charlie: {s:?}");
}

fn selected_tail(s: &str) -> String {
    s.chars().rev().take(2).collect::<String>().chars().rev().collect()
}

#[test]
fn a_handle_moves_the_line_above_the_finger_not_the_line_under_it() {
    let mut rt = rig();
    let r = rect(&rt, "lines");
    // "first" on the first line.
    let (x, y) = (r[0] + 12.0, r[1] + 10.0);
    jittering_press(&mut rt, x, y);
    assert_eq!(selected(&rt.state.lines), "first");
    let [_, (ex, ey)] = rt.selection_handles().expect("handles");
    // Held below the first line, over the second, the end stays on the first line.
    rt.touch(2, TouchPhase::Started, ex + 2.0, ey + 12.0, 1.0);
    rt.touch(2, TouchPhase::Moved, ex + 60.0, ey + 12.0, 1.1);
    rt.frame(0.05);
    rt.touch(2, TouchPhase::Ended, ex + 60.0, ey + 12.0, 1.2);
    rt.frame(0.05);
    let s = selected(&rt.state.lines).to_owned();
    assert!(s.starts_with("first li") && !s.contains('\n'), "the end moved along the first line: {s:?}");
}

#[test]
fn a_long_press_after_a_tap_elsewhere_in_the_field_selects_a_word_not_a_line() {
    let mut rt = rig();
    let r = rect(&rt, "field");
    let y = r[1] + r[3] / 2.0;
    rt.touch(1, TouchPhase::Started, x_of(&rt, "alpha"), y, 0.0);
    rt.touch(1, TouchPhase::Ended, x_of(&rt, "alpha"), y, 0.05);
    rt.frame(0.05);
    let x = x_of(&rt, "delta");
    jittering_press(&mut rt, x, y);
    assert_eq!(selected(&rt.state.field), "delta");
    // A tap on the word, clear of its handles, places a caret: the long press was not the first
    // of a triple click.
    let b = rt.selection_boxes()[0];
    let (x, y) = (b[0] + b[2] / 2.0, b[1] + 3.0);
    rt.touch(3, TouchPhase::Started, x, y, 0.9);
    rt.touch(3, TouchPhase::Ended, x, y, 0.95);
    rt.frame(0.05);
    assert_eq!(selected(&rt.state.field), "", "a tap on the selection places the caret");
}

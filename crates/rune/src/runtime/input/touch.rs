//! Touchscreen and pen input, as a browser handles it: the first finger is the primary
//! button. It presses where it lands, so a tap clicks; dragging an element that drags
//! (`El::on_drag`: a slider, a scroll bar) drags it; any other drag pans the scroll viewport
//! under the finger (`runtime/scroll/touch.rs`), flinging it on with momentum, and cancels
//! the press. A second finger makes the two a pinch: the first finger's press and pan are
//! let go, and the change in the fingers' distance reaches the `El::on_magnify` handlers
//! under their midpoint, as a trackpad's pinch does, until either lifts. Further fingers are
//! ignored meanwhile. A finger held still selects the word under it, or opens the context menu
//! of what it is on where that is not selectable text (`touch_select.rs`); one that left the
//! slop first is a drag or a pan and never becomes a long press.
use super::*;

#[path = "touch_select.rs"]
mod touch_select;
use crate::{GesturePhase, MagnifyEvent};

/// How far (points) a finger travels before its touch is a drag or a pan, not a tap.
const TOUCH_SLOP: f32 = 8.0;
/// A finger held this long (seconds) before lifting throws nothing.
const FLING_HOLD: f64 = 0.1;

/// A host's touch phase, as winit reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    /// The system took the touch (a gesture of its own): nothing it pressed is clicked.
    Cancelled,
}

/// What touches are doing: the finger that is down, and the viewport a touch holds or flung.
#[derive(Default)]
pub(super) struct Touch {
    pub(super) scroll: Option<scroll_touch::TouchScroll>,
    finger: Option<Finger>,
    pinch: Option<Pinch>,
    /// When (host time on the runtime clock) the finger down now selects instead of pressing.
    long_due: Option<f64>,
    /// The host's long-press time in seconds (`Runtime::set_long_press_timeout`), else
    /// `touch_select::LONG_PRESS`.
    long_press: Option<f64>,
}

/// Two fingers pinching: where each is, and their distance at the last event.
struct Pinch {
    fingers: [(u64, (f32, f32)); 2],
    distance: f32,
}

impl Pinch {
    fn middle(&self) -> (f32, f32) {
        let [(_, a), (_, b)] = self.fingers;
        ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
    }

    fn spread(&self) -> f32 {
        let [(_, a), (_, b)] = self.fingers;
        (a.0 - b.0).hypot(a.1 - b.1)
    }
}

struct Finger {
    id: u64,
    origin: (f32, f32),
    /// Where it is now, for a pinch a second finger starts.
    at_point: (f32, f32),
    last: f32,
    at: f64,
    /// Points per second along the pan's axis.
    velocity: f32,
    mode: Mode,
    /// The vertical viewport under the finger, held until the pan picks its axis.
    scroll: Option<NodeId>,
    horizontal: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Still within the slop: lifting it here is a tap.
    Pending,
    Drag,
    Pan,
    /// Selecting text: after a long press, or holding a selection handle.
    Select,
    /// Its long press opened a context menu: it presses and moves nothing more.
    Held,
}

impl<S: 'static> Runtime<S> {
    /// One touch event at (x, y) in window points, `time` in seconds on any steady clock. A
    /// host with touches calls this for every one; `id` tells fingers apart.
    pub fn touch(&mut self, id: u64, phase: TouchPhase, x: f32, y: f32, time: f64) {
        match phase {
            TouchPhase::Started => self.touch_down(id, x, y, time),
            TouchPhase::Moved => self.touch_move(id, x, y, time),
            TouchPhase::Ended | TouchPhase::Cancelled => self.touch_up(id, x, y, time, phase == TouchPhase::Cancelled),
        }
    }

    /// Whether a finger is down: a host ignores the mouse events its system makes up from it.
    pub fn touching(&self) -> bool {
        self.touch.finger.is_some() || self.touch.pinch.is_some()
    }

    /// The surface went away under a finger: end it as a cancel, so the press doesn't stay down
    /// waiting for a lift that won't come. (From maxmaster55/rune `claude/android-demo`.)
    pub fn cancel_touch(&mut self) {
        let Some(f) = self.touch.finger.as_ref() else { return };
        let (id, x, y, time) = (f.id, f.origin.0, f.origin.1, f.at);
        self.touch_up(id, x, y, time, true);
    }

    fn touch_down(&mut self, id: u64, x: f32, y: f32, time: f64) {
        if self.touch.pinch.is_some() { return; }
        if let Some(first) = self.touch.finger.take_if(|f| f.id != id) {
            // A second finger: what the first pressed or panned is let go, unclicked.
            self.touch.long_due = None;
            self.touch_scroll_end(first.scroll, 0.0);
            self.cancel_pointer();
            self.pointer_leave();
            let pinch = Pinch { fingers: [(first.id, first.at_point), (id, (x, y))], distance: 0.0 };
            let distance = pinch.spread();
            let (mx, my) = pinch.middle();
            self.touch.pinch = Some(Pinch { distance, ..pinch });
            self.magnify_event(MagnifyEvent::new(0.0).at(mx, my).phase(GesturePhase::Began));
            return;
        }
        if self.touch.finger.is_some() { return; }
        if self.take_handle(x, y) {
            self.touch.finger = Some(Finger { id, origin: (x, y), at_point: (x, y), last: 0.0, at: time, velocity: 0.0, mode: Mode::Select, scroll: None, horizontal: false });
            return;
        }
        let scroll = self.touch_scroll_start(x, y);
        self.pointer_move(x, y);
        self.pointer_down(x, y);
        self.touch.finger = Some(Finger { id, origin: (x, y), at_point: (x, y), last: 0.0, at: time, velocity: 0.0, mode: Mode::Pending, scroll, horizontal: false });
        self.arm_long_press();
    }

    /// A pinching finger moved: the change in spread since the last event, at the middle.
    fn pinch_move(&mut self, id: u64, x: f32, y: f32) {
        let Some(pinch) = self.touch.pinch.as_mut() else { return };
        let Some(finger) = pinch.fingers.iter_mut().find(|(f, _)| *f == id) else { return };
        finger.1 = (x, y);
        let (spread, before) = (pinch.spread(), pinch.distance);
        let (mx, my) = pinch.middle();
        // Fingers on top of each other measure nothing to scale by.
        if before < 1.0 || spread < 1.0 { return; }
        pinch.distance = spread;
        self.magnify_event(MagnifyEvent::new(spread / before - 1.0).at(mx, my).phase(GesturePhase::Moved));
    }

    fn touch_move(&mut self, id: u64, x: f32, y: f32, time: f64) {
        if self.touch.pinch.is_some() { return self.pinch_move(id, x, y); }
        if let Some(f) = self.touch.finger.as_mut().filter(|f| f.id == id) { f.at_point = (x, y); }
        let Some(f) = self.touch.finger.as_ref().filter(|f| f.id == id) else { return };
        let (dx, dy) = (x - f.origin.0, y - f.origin.1);
        if f.mode == Mode::Pending && dx.hypot(dy) > TOUCH_SLOP {
            let (origin, scroll) = (f.origin, f.scroll);
            // Past the slop it is a drag or a pan for good: no long press is still to come.
            self.touch.long_due = None;
            let (scroll, horizontal) = if self.press_drags() { (None, false) } else { self.touch_scroll_pan(origin.0, origin.1, dx.abs() > dy.abs(), scroll) };
            let f = self.touch.finger.as_mut().unwrap();
            (f.scroll, f.horizontal, f.mode) = (scroll, horizontal, if scroll.is_some() { Mode::Pan } else { Mode::Drag });
        }
        let f = self.touch.finger.as_mut().unwrap();
        let along = if f.horizontal { dx } else { dy };
        let dt = time - f.at;
        if dt > 0.0 { f.velocity = (along - f.last) / dt as f32; }
        (f.last, f.at) = (along, time);
        match (f.mode, f.scroll) {
            (Mode::Pan, Some(scroll)) => self.touch_scroll_move(scroll, along),
            (Mode::Drag, _) => self.pointer_move(x, y),
            (Mode::Select, _) => self.selection_drag(x, y),
            _ => {}
        }
    }

    fn touch_up(&mut self, id: u64, x: f32, y: f32, time: f64, cancelled: bool) {
        if let Some(pinch) = self.touch.pinch.take_if(|p| p.fingers.iter().any(|(f, _)| *f == id)) {
            // Either finger lifting ends it; the other presses nothing when it lifts.
            let (mx, my) = pinch.middle();
            self.magnify_event(MagnifyEvent::new(0.0).at(mx, my).phase(GesturePhase::Ended));
            return;
        }
        let Some(f) = self.touch.finger.take_if(|f| f.id == id) else { return };
        self.touch.long_due = None;
        if f.mode == Mode::Held {
            // The menu it opened stays; the press was let go when it opened.
            self.pointer_leave();
            return;
        }
        if f.mode == Mode::Select {
            // The selection stays, cancelled or not; nothing was pressed to release.
            self.text_sel.dragging = false;
            self.pointer_leave();
            return;
        }
        let pan = f.mode == Mode::Pan;
        let velocity = if pan && !cancelled && time - f.at < FLING_HOLD { f.velocity } else { 0.0 };
        self.touch_scroll_end(f.scroll, velocity);
        if cancelled {
            self.cancel_pointer();
            return;
        }
        // A pan cancelled the press, so its release clicks nothing.
        let (x, y) = if pan { f.origin } else { (x, y) };
        if f.mode == Mode::Drag { self.pointer_move(x, y); }
        self.pointer_up(x, y);
        // No hover is left under a lifted finger.
        self.pointer_leave();
    }

    /// Whether the held press is on an element that drags, so a finger drags it, not the page.
    fn press_drags(&self) -> bool {
        let Some((path, _)) = &self.capture else { return false };
        self.live_index(path).is_some_and(|i| self.nodes[i].handlers.on_drag.is_some() && !self.drags_out(&self.nodes[i]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ScrollSpec, col, el};

    #[derive(Default)]
    struct S { taps: Vec<usize>, drags: Vec<f32> }

    fn view(_: &S, _: &mut Cx<S>) -> El {
        col().size(200.0, 300.0)
            .child(el().key("knob").size(200.0, 40.0).on_drag(|s: &mut S, d: &mut Drag, _| s.drags.push(d.dx)))
            .child(col().key("list").w_full().h(200.0).scroll(ScrollSpec { stick_bottom: false })
                .children((0..30).map(|i| el().key(format!("row{i}")).w_full().h(40.0).on_press(move |s: &mut S| s.taps.push(i)))))
    }

    fn rig() -> Runtime<S> {
        let mut rt = Runtime::new(S::default(), view, (200.0, 300.0), Env::default());
        rt.frame(0.0);
        rt
    }

    fn offset(rt: &Runtime<S>) -> f64 {
        let id = rt.nodes.iter().find(|n| n.key.as_deref() == Some("list")).unwrap().id;
        rt.scrolls[&id].offset
    }

    /// A finger from (x, y) moving by `dy` over `steps` events 10 ms apart, then lifted.
    fn swipe(rt: &mut Runtime<S>, (x, y): (f32, f32), dy: f32, steps: usize) {
        rt.touch(1, TouchPhase::Started, x, y, 0.0);
        for i in 1..=steps {
            rt.touch(1, TouchPhase::Moved, x, y + dy * i as f32 / steps as f32, i as f64 * 0.01);
        }
        rt.touch(1, TouchPhase::Ended, x, y + dy, steps as f64 * 0.01 + 0.005);
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn a_tap_clicks_what_it_lands_on_and_leaves_no_hover() {
        let mut rt = rig();
        rt.touch(1, TouchPhase::Started, 100.0, 70.0, 0.0);
        rt.touch(1, TouchPhase::Moved, 103.0, 72.0, 0.02);
        rt.touch(1, TouchPhase::Ended, 103.0, 72.0, 0.05);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.taps, [0]);
        assert_eq!(rt.pointer(), None);
        assert!(!rt.touching());
    }

    #[test]
    fn a_drag_on_a_list_scrolls_it_without_clicking_and_flings_on() {
        let mut rt = rig();
        swipe(&mut rt, (100.0, 200.0), -100.0, 5);
        assert!(rt.state.taps.is_empty(), "a pan presses nothing");
        let lifted = offset(&rt);
        assert!(lifted >= 90.0, "the list followed the finger: {lifted}");
        for _ in 0..30 { rt.frame(1.0 / 60.0); }
        assert!(offset(&rt) > lifted + 50.0, "and coasted on: {} after {lifted}", offset(&rt));
    }

    #[test]
    fn a_finger_held_still_before_lifting_throws_nothing() {
        let mut rt = rig();
        rt.touch(1, TouchPhase::Started, 100.0, 200.0, 0.0);
        rt.touch(1, TouchPhase::Moved, 100.0, 150.0, 0.01);
        rt.touch(1, TouchPhase::Ended, 100.0, 150.0, 0.5);
        for _ in 0..30 { rt.frame(1.0 / 60.0); }
        assert_eq!(offset(&rt), 50.0);
    }

    #[test]
    fn a_drag_on_an_element_that_drags_drags_it_not_the_page() {
        let mut rt = rig();
        rt.touch(1, TouchPhase::Started, 20.0, 20.0, 0.0);
        rt.touch(1, TouchPhase::Moved, 60.0, 30.0, 0.01);
        rt.touch(1, TouchPhase::Ended, 60.0, 30.0, 0.02);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.drags.last(), Some(&40.0));
        assert_eq!(offset(&rt), 0.0);
    }

    #[test]
    fn a_second_finger_is_ignored_and_a_cancelled_touch_clicks_nothing() {
        let mut rt = rig();
        rt.touch(1, TouchPhase::Started, 100.0, 70.0, 0.0);
        rt.touch(2, TouchPhase::Started, 100.0, 110.0, 0.0);
        rt.touch(2, TouchPhase::Ended, 100.0, 110.0, 0.01);
        rt.touch(1, TouchPhase::Cancelled, 100.0, 70.0, 0.02);
        rt.frame(1.0 / 60.0);
        assert!(rt.state.taps.is_empty());
        assert!(!rt.touching());
    }

    #[derive(Default)]
    struct Zoom { taps: usize, scale: f32, phases: Vec<GesturePhase> }

    fn zoomable(_: &Zoom, _: &mut Cx<Zoom>) -> El {
        el().key("canvas").size(300.0, 300.0)
            .on_press(|s: &mut Zoom| s.taps += 1)
            .on_magnify(|s: &mut Zoom, e: MagnifyEvent, _| { s.scale *= 1.0 + e.delta; s.phases.push(e.phase); true })
    }

    #[test]
    fn two_fingers_pinch_the_magnify_handlers_and_press_nothing() {
        let mut rt = Runtime::new(Zoom { scale: 1.0, ..Zoom::default() }, zoomable, (300.0, 300.0), Env::default());
        rt.frame(0.0);
        rt.touch(1, TouchPhase::Started, 100.0, 150.0, 0.0);
        rt.touch(2, TouchPhase::Started, 200.0, 150.0, 0.01);
        assert!(rt.touching());
        for step in 1..=4 {
            let spread = 50.0 + 25.0 * step as f32;
            rt.touch(1, TouchPhase::Moved, 150.0 - spread, 150.0, 0.01 + step as f64 * 0.01);
            rt.touch(2, TouchPhase::Moved, 150.0 + spread, 150.0, 0.015 + step as f64 * 0.01);
            rt.frame(1.0 / 60.0);
        }
        rt.touch(2, TouchPhase::Ended, 300.0, 150.0, 0.1);
        rt.touch(1, TouchPhase::Moved, 10.0, 150.0, 0.11);
        rt.touch(1, TouchPhase::Ended, 10.0, 150.0, 0.12);
        rt.frame(1.0 / 60.0);
        assert!((rt.state.scale - 3.0).abs() < 0.01, "the spread went from 100 to 300 points: {}", rt.state.scale);
        assert_eq!(rt.state.phases.first(), Some(&GesturePhase::Began));
        assert_eq!(rt.state.phases.last(), Some(&GesturePhase::Ended));
        assert_eq!(rt.state.taps, 0, "neither finger clicked what it landed on");
        assert!(!rt.touching());
        // One finger again is a tap.
        rt.touch(3, TouchPhase::Started, 150.0, 150.0, 1.0);
        rt.touch(3, TouchPhase::Ended, 150.0, 150.0, 1.05);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.taps, 1);
    }

    #[test]
    fn a_second_finger_ends_the_first_ones_pan_without_a_fling() {
        let mut rt = rig();
        rt.touch(1, TouchPhase::Started, 100.0, 250.0, 0.0);
        rt.touch(1, TouchPhase::Moved, 100.0, 200.0, 0.01);
        let panned = offset(&rt);
        rt.touch(2, TouchPhase::Started, 150.0, 250.0, 0.02);
        rt.touch(1, TouchPhase::Moved, 100.0, 100.0, 0.03);
        rt.touch(1, TouchPhase::Ended, 100.0, 100.0, 0.035);
        rt.touch(2, TouchPhase::Ended, 150.0, 250.0, 0.04);
        for _ in 0..30 { rt.frame(1.0 / 60.0); }
        assert_eq!(offset(&rt), panned, "the list stays where the pan left it");
        assert!(rt.state.taps.is_empty());
    }
}


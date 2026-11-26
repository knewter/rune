//! What a finger shows, as Android's and UIKit's views show it. A finger never hovers: nothing
//! takes its hover look from a touch. A finger that lands on a control inside something that may
//! take the gesture from it -- a viewport that scrolls, an element that swipes (`El::on_swipe`:
//! a drawer, a gallery), or one that drags (`El::on_drag`: a sheet, a swipeable row) -- shows the
//! control pressed only once it has rested there the tap timeout ([`TAP_TIMEOUT`], or the host's
//! own: [`Runtime::set_tap_timeout`]), as Android's `ViewConfiguration.getTapTimeout()` and
//! UIKit's `delaysContentTouches` wait to see whether the touch is a tap or a scroll. A finger
//! that leaves the slop before then shows no press at all, and one the scroll or swipe takes lets
//! the press go unclicked, as it always has. A tap that lifts before the timeout still presses
//! and flashes the press as it lets go. Where nothing above can take the gesture the press shows
//! at once. The press itself (its handlers, focus, a drag's `Phase::Down`) is never delayed: only
//! how it looks.
use super::*;

/// How long (seconds) a finger rests on a control inside something that may take its gesture
/// before the control shows pressed: Android's `ViewConfiguration.TAP_TIMEOUT`.
pub(super) const TAP_TIMEOUT: f64 = 0.1;

impl<S: 'static> Runtime<S> {
    /// How long a finger rests on a control inside a scroll, swipe or drag before the control
    /// shows pressed, in seconds: a host passes its platform's (Android's
    /// `ViewConfiguration.getTapTimeout()`). Without it, [`TAP_TIMEOUT`]. At most half a second.
    pub fn set_tap_timeout(&mut self, secs: f64) {
        self.touch.tap_timeout = secs.is_finite().then(|| secs.clamp(0.0, 0.5));
    }

    /// The tap timeout in force.
    pub fn tap_timeout(&self) -> f64 {
        self.touch.tap_timeout.unwrap_or(TAP_TIMEOUT)
    }

    /// How far (points) a finger travels before its touch is a drag, a pan or a swipe and not a
    /// tap: a host passes its platform's (Android's `ViewConfiguration.getScaledTouchSlop()` over
    /// the display density). Without it, 8 points. Between 2 and 48.
    pub fn set_touch_slop(&mut self, points: f32) {
        self.touch.slop = points.is_finite().then(|| points.clamp(2.0, 48.0));
    }

    /// The touch slop in force.
    pub fn touch_slop(&self) -> f32 {
        self.touch.slop.unwrap_or(TOUCH_SLOP)
    }

    /// Whether the pointer is a finger (down, pinching or lifting): its hover shows nothing.
    pub(in crate::runtime) fn finger_pointer(&self) -> bool {
        self.touch.finger.is_some() || self.touch.pinch.is_some() || self.touch.lifting
    }

    /// Whether the finger's press is held back from showing.
    pub(in crate::runtime) fn press_held_back(&self) -> bool {
        self.touch.press_due.is_some()
    }

    /// A finger pressed at (x, y), `scroll` the viewport it caught: hold the press's look back
    /// the tap timeout when something above what it pressed may take the gesture.
    pub(super) fn hold_back_press(&mut self, x: f32, y: f32, scroll: Option<NodeId>) {
        self.touch.press_due = None;
        let Some(i) = self.capture.as_ref().and_then(|(path, _)| self.live_index(path)) else { return };
        if !self.gesture_above(i, x, y, scroll) { return; }
        let (now, after) = (self.now_clock(), self.tap_timeout());
        if after <= 0.0 { return; }
        self.touch.press_due = Some(now + after);
        self.schedule_wakes_from(now, [std::time::Duration::from_secs_f64(after)]);
    }

    /// Whether something above node `i` may take a finger's gesture at (x, y) from it.
    fn gesture_above(&self, i: usize, x: f32, y: f32, scroll: Option<NodeId>) -> bool {
        let sideways = self.scroll_at_axis(x, y, true).and_then(|id| self.scrolls.get(&id)).is_some_and(|st| st.x.max > 0.5);
        scroll.is_some() || sideways || self.swipe_under(x, y).is_some()
            || std::iter::successors(self.nodes[i].parent, |&p| self.nodes[p].parent)
                .any(|p| self.nodes[p].handlers.on_drag.is_some())
    }

    /// A frame, or the finger's next event, at or after the tap timeout, the finger still
    /// resting: the press shows.
    pub(in crate::runtime) fn fire_press_due(&mut self) {
        let Some(due) = self.touch.press_due else { return };
        if self.now_clock() + 1e-6 < due { return; }
        self.touch.press_due = None;
    }

    /// The finger left the slop: a press still held back never shows.
    pub(super) fn press_never_shows(&mut self) {
        if self.touch.press_due.is_some() { self.touch.press_due = Some(f64::INFINITY); }
    }

    /// A tap lifts before its press showed: the press flashes as it lets go, so the tap is seen.
    pub(super) fn flash_held_back_press(&mut self) {
        let Some(due) = self.touch.press_due.take() else { return };
        if !due.is_finite() || !self.capture_inside { return; }
        let Some(i) = self.capture.as_ref().and_then(|(path, _)| self.live_index(path)) else { return };
        if self.nodes[i].vis.press.a > 0.0 && self.allowed(i) {
            self.scene.set(self.nodes[i].id, P_PRESS, 1.0);
        }
    }
}

#[cfg(test)]
#[path = "touch_press_tests.rs"]
mod tests;

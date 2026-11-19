//! Selecting text by touch, as Android's text views do. A finger held still on
//! [`El::selectable`](crate::El::selectable) text for [`LONG_PRESS`] selects the word under it
//! instead of pressing what it landed on, and moving it then extends the selection by words.
//! With a selection made, a finger that lands on one of its ends -- where a view draws the
//! handles, under the first line box's start and the last's end
//! ([`Cx::selection_rects`](crate::Cx::selection_rects)) -- moves that end and leaves the other
//! where it is. Copying is the view's: a toolbar of
//! [`El::keeps_text_selection`](crate::El::keeps_text_selection) controls over those boxes.
use super::*;

/// How long (seconds) a finger is held still before it selects instead of pressing.
pub(super) const LONG_PRESS: f64 = 0.5;
/// How far (points) from the point a handle hangs from a finger still takes it: the handle is
/// drawn below the line, so its reach is centred half of this below the point.
pub(crate) const HANDLE_REACH: f32 = 24.0;

impl<S: 'static> Runtime<S> {
    /// A finger came down: its long press is due [`LONG_PRESS`] from now.
    pub(super) fn arm_long_press(&mut self) {
        self.touch.long_due = Some(self.clock + LONG_PRESS);
        self.schedule_wakes([std::time::Duration::from_secs_f64(LONG_PRESS)]);
    }

    /// A frame at or after the long press's time, the finger still within the slop: on
    /// selectable text (away from its links) the press is let go unclicked, the viewport it held
    /// is left where it is, and the word under the finger is selected.
    pub(in crate::runtime) fn fire_long_press(&mut self) {
        let Some(due) = self.touch.long_due else { return };
        if self.clock + 1e-6 < due { return; }
        self.touch.long_due = None;
        let Some(f) = self.touch.finger.as_ref().filter(|f| f.mode == Mode::Pending) else { return };
        let ((x, y), scroll) = (f.origin, f.scroll);
        let Some(i) = self.selectable_at(x, y).filter(|&i| self.link_at(i, x, y).is_none()) else { return };
        self.touch_scroll_end(scroll, 0.0);
        self.cancel_pointer();
        // The press's second click selects a word: a long press is that click, at once.
        self.text_sel.shift = false;
        self.text_sel.last = Some((self.nodes[i].path.clone(), self.now_clock(), x, y, 1));
        self.selection_down(Some(i), x, y);
        if let Some(f) = self.touch.finger.as_mut() { (f.mode, f.scroll) = (Mode::Select, None); }
    }

    /// Where the selection's handles hang from, in window points: under the start of its first
    /// line box and the end of its last. None with no selection or only a caret.
    pub fn selection_handles(&self) -> Option<[(f32, f32); 2]> {
        let boxes = self.selection_boxes();
        let (first, last) = (boxes.first()?, boxes.last()?);
        Some([(first[0], first[1] + first[3]), (last[0] + last[2], last[1] + last[3])])
    }

    /// A finger came down at (x, y): on a handle it takes that end of the selection, which then
    /// follows the finger while the other stays. True when it did, and nothing is pressed.
    pub(super) fn take_handle(&mut self, x: f32, y: f32) -> bool {
        let Some(handles) = self.selection_handles() else { return false };
        let reach = |(hx, hy): (f32, f32)| (x - hx).hypot(y - (hy + HANDLE_REACH / 2.0));
        let (start, end) = (reach(handles[0]), reach(handles[1]));
        if start.min(end) > HANDLE_REACH { return false; }
        // The end that moves is the focus; the focus is at the start when it was made backwards.
        let moving_start = start < end;
        if moving_start != self.selection_backward() { self.swap_selection_ends(); }
        let sel = &mut self.text_sel;
        sel.unit = crate::runtime::selectable::Unit::Char;
        sel.origin = (sel.anchor, sel.anchor);
        sel.dragging = true;
        self.seal_between();
        true
    }

    /// The anchor becomes the focus and the focus the anchor, in whichever texts they are.
    fn swap_selection_ends(&mut self) {
        let sel = &mut self.text_sel;
        std::mem::swap(&mut sel.anchor, &mut sel.focus);
        std::mem::swap(&mut sel.anchor_end, &mut sel.focus_end);
        if let Some((path, content)) = sel.far.take() {
            let here = sel.path.replace(path).unwrap_or_default();
            sel.far = Some((here, std::mem::replace(&mut sel.content, content)));
        }
    }
}

#[cfg(test)]
#[path = "touch_select_tests.rs"]
mod tests;

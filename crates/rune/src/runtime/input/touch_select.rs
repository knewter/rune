//! Selecting text by touch, as Android's text views do. A finger held still on
//! [`El::selectable`](crate::El::selectable) text for the long-press time ([`LONG_PRESS`], or the
//! host's own: [`Runtime::set_long_press_timeout`]) selects the word under it instead of pressing
//! what it landed on, and moving it then extends the selection by words. The time is counted
//! from the touch on the host's clock, not from the last frame: an idle window's clock is as old
//! as its last frame, and a long press measured from there was due the moment the finger landed.
//! A finger that leaves the slop before then pans or drags and never selects. Held still on
//! anything else, it opens that element's context menu (`El::on_secondary_press`), as a
//! right-click does.
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
    /// How long a finger is held still before its long press, in seconds: a host passes its
    /// platform's (Android's `ViewConfiguration.getLongPressTimeout()`, which a person can
    /// lengthen). Without it, [`LONG_PRESS`]. Never under a tenth of a second.
    pub fn set_long_press_timeout(&mut self, secs: f64) {
        self.touch.long_press = secs.is_finite().then(|| secs.clamp(0.1, 5.0));
    }

    /// The long-press time in force.
    pub fn long_press_timeout(&self) -> f64 {
        self.touch.long_press.unwrap_or(LONG_PRESS)
    }

    /// A finger came down: its long press is due the long-press time from now, on the host's
    /// clock. `self.clock` is the last frame's time, which is long past under an idle window.
    pub(super) fn arm_long_press(&mut self) {
        let (now, after) = (self.now_clock(), self.long_press_timeout());
        self.touch.long_due = Some(now + after);
        self.schedule_wakes_from(now, [std::time::Duration::from_secs_f64(after)]);
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
        let Some(i) = self.selectable_at(x, y) else { return self.long_press_menu(x, y, scroll) };
        if self.link_at(i, x, y).is_some() { return; }
        self.touch_scroll_end(scroll, 0.0);
        self.cancel_pointer();
        // The press's second click selects a word: a long press is that click, at once.
        self.text_sel.shift = false;
        self.text_sel.last = Some((self.nodes[i].path.clone(), self.now_clock(), x, y, 1));
        self.selection_down(Some(i), x, y);
        if let Some(f) = self.touch.finger.as_mut() { (f.mode, f.scroll) = (Mode::Select, None); }
    }

    /// A long press off selectable text: the context menu of what is under the finger
    /// (`El::on_secondary_press`), as a right-click there opens it. The press is let go
    /// unclicked. With no menu there the press stays, and lifting still clicks.
    fn long_press_menu(&mut self, x: f32, y: f32, scroll: Option<NodeId>) {
        let Some(path) = self.secondary_target(x, y).map(str::to_owned) else { return };
        if !self.live_index(&path).is_some_and(|i| self.nodes[i].handlers.on_secondary_press.is_some()) { return; }
        self.touch_scroll_end(scroll, 0.0);
        self.cancel_pointer();
        self.secondary_down(x, y);
        self.secondary_up(x, y);
        self.pointer_leave();
        if let Some(f) = self.touch.finger.as_mut() { (f.mode, f.scroll) = (Mode::Held, None); }
    }

    /// The part of the window (x0, y0, x1, y1) the selection's texts show in: the clips around
    /// its first text (the scroll viewport it is read in). None with no selection.
    pub fn selection_clip(&self) -> Option<[f32; 4]> {
        let (i, ..) = *self.selection_ranges().first()?;
        if self.inh_scratch.len() != self.nodes.len() { return None; }
        Some(self.screen_clip_of_parent(Some(i)).map(|v| v / self.env.scale))
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
        // A handle scrolled out of its viewport is not drawn, and takes no finger: a touch on
        // what is drawn there (a composer under the transcript) is that control's.
        let clip = self.selection_clip();
        let shown = |(hx, hy): (f32, f32)| clip.is_none_or(|c| hx >= c[0] - 0.5 && hx <= c[2] + 0.5 && hy >= c[1] - 0.5 && hy <= c[3] + 0.5);
        let reach = |h: (f32, f32)| if shown(h) { (x - h.0).hypot(y - (h.1 + HANDLE_REACH / 2.0)) } else { f32::INFINITY };
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

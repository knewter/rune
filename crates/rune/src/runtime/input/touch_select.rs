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

    /// A frame, or the finger's next event, at or after the long press's time (on the host's
    /// clock, so a host that drew no frame in between still hears of it), the finger still
    /// within the slop: on
    /// selectable text (away from its links) the press is let go unclicked, the viewport it held
    /// is left where it is, and the word under the finger is selected.
    pub(in crate::runtime) fn fire_long_press(&mut self) {
        let Some(due) = self.touch.long_due else { return };
        if self.now_clock() + 1e-6 < due { return; }
        self.touch.long_due = None;
        let Some(f) = self.touch.finger.as_ref().filter(|f| f.mode == Mode::Pending) else { return };
        let ((x, y), scroll) = (f.origin, f.scroll);
        if let Some(j) = self.field_at(x, y) { return self.long_press_field(j, x, y, scroll); }
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

    /// The text field under (x, y), if it is the topmost thing there that takes input.
    fn field_at(&self, x: f32, y: f32) -> Option<usize> {
        self.top_target(x, y).filter(|&i| self.nodes[i].input.is_some() && !self.nodes[i].inert)
    }

    /// A long press in a text field: the word under the finger is selected, as a double click
    /// selects it, and moving the finger then extends the selection by words. The press it began
    /// with (which focused the field and placed its caret) is that click's first half.
    fn long_press_field(&mut self, j: usize, x: f32, y: f32, scroll: Option<NodeId>) {
        self.touch_scroll_end(scroll, 0.0);
        let now = self.now_clock();
        self.input_click.press_again(&self.nodes[j].path, now, x, y);
        self.input_down(j, x, y);
        if let Some(f) = self.touch.finger.as_mut() { (f.mode, f.scroll) = (Mode::Select, None); }
    }

    /// A finger selecting moved to (x, y): the end it holds follows the point it holds it by,
    /// in a field or in selectable text.
    pub(super) fn touch_select_move(&mut self, x: f32, y: f32) {
        let Some(f) = self.touch.finger.as_ref() else { return };
        let (x, y) = (x - f.grab.0, y - f.grab.1);
        if self.input_click.dragging_path().is_some() { self.input_drag(x, y, true) } else { self.selection_drag(x, y) }
    }

    /// A finger selecting lifted: the selection stays and nothing is released or clicked. A
    /// field's press ends without its click count, so a tap next places a caret.
    pub(super) fn touch_select_up(&mut self) {
        self.text_sel.dragging = false;
        if self.input_click.dragging_path().is_some() || self.input_click.captured {
            self.input_click.end_press();
            self.input_click.clear_last();
            self.capture = None;
        }
        self.pointer_leave();
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
        if self.inh_scratch.len() != self.nodes.len() { return None; }
        let Some(&(i, ..)) = self.selection_ranges().first() else {
            // A field's range shows inside the field, within the clips around it.
            let i = self.selected_field(&self.nodes)?;
            let [x0, y0, x1, y1] = self.screen_clip_of_parent(Some(i)).map(|v| v / self.env.scale);
            let t = self.nodes[i].target;
            let inset = self.nodes[i].inset;
            return Some([x0.max(t[0] + inset[0]), y0.max(t[1]), x1.min(t[2] - inset[2]), y1.min(t[3])]);
        };
        Some(self.screen_clip_of_parent(Some(i)).map(|v| v / self.env.scale))
    }

    /// Whether the selection is the focused text field's range (`Cx::selection_in_field`).
    pub fn selection_in_field(&self) -> bool {
        self.selection_ranges().is_empty() && self.selected_field(&self.nodes).is_some()
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
    pub(super) fn take_handle(&mut self, x: f32, y: f32) -> Option<(f32, f32)> {
        let handles = self.selection_handles()?;
        // A handle scrolled out of its viewport is not drawn, and takes no finger: a touch on
        // what is drawn there (a composer under the transcript) is that control's.
        let clip = self.selection_clip();
        let shown = |(hx, hy): (f32, f32)| clip.is_none_or(|c| hx >= c[0] - 0.5 && hx <= c[2] + 0.5 && hy >= c[1] - 0.5 && hy <= c[3] + 0.5);
        let reach = |h: (f32, f32)| if shown(h) { (x - h.0).hypot(y - (h.1 + HANDLE_REACH / 2.0)) } else { f32::INFINITY };
        let (start, end) = (reach(handles[0]), reach(handles[1]));
        if start.min(end) > HANDLE_REACH { return None; }
        let moving_start = start < end;
        // The finger holds the handle below the line it hangs from: the end it moves is where
        // the middle of that line would be, wherever the finger is (Android's handles keep the
        // finger the same way), so it never drops to the line under the finger.
        let boxes = self.selection_boxes();
        let line = if moving_start { boxes.first() } else { boxes.last() }.map_or(0.0, |b| b[3]);
        let (hx, hy) = handles[usize::from(!moving_start)];
        let grab = (x - hx, y - (hy - line / 2.0));
        if let Some(j) = self.selected_field(&self.nodes) {
            // A field's end moves through its own edits: the other end is held where it is.
            let edit = self.nodes[j].input.as_ref().map(|inp| inp.edit.range())?;
            let fixed = if moving_start { edit.1 } else { edit.0 };
            let path = self.nodes[j].path.clone();
            self.press_at = (x, y);
            self.input_click.hold_end(path, fixed);
            return Some(grab);
        }
        // The end that moves is the focus; the focus is at the start when it was made backwards.
        if moving_start != self.selection_backward() { self.swap_selection_ends(); }
        let sel = &mut self.text_sel;
        sel.unit = crate::runtime::selectable::Unit::Char;
        sel.origin = (sel.anchor, sel.anchor);
        sel.dragging = true;
        self.seal_between();
        Some(grab)
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

#[cfg(test)]
#[path = "touch_field_tests.rs"]
mod field_tests;

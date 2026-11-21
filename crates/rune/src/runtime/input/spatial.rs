//! Arrow keys that move focus by where controls are drawn, as a television's D-pad does
//! (Android's `FocusFinder`): Right goes to the nearest focusable control to the right, not
//! to the next one in Tab order. Off unless the host asks (`Runtime::set_spatial_arrows`).
use super::*;
use crate::Key;

/// Which way an arrow points, on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Toward { Left, Right, Up, Down }

impl Toward {
    pub(super) fn of(key: &Key) -> Option<Self> {
        Some(match key {
            Key::Left => Self::Left, Key::Right => Self::Right,
            Key::Up => Self::Up, Key::Down => Self::Down,
            _ => return None,
        })
    }
}

/// The candidate nearest `from` toward `dir`, of `rects` (`[x, y, w, h]`), as `FocusFinder`
/// chooses: a rect wholly or partly beyond `from`'s edge, those sharing `from`'s row (or
/// column) first, then by `13 * along² + across²`, where `along` is the gap past the edge
/// and `across` the distance between centres the other way.
pub(super) fn nearest(from: [f32; 4], dir: Toward, rects: impl IntoIterator<Item = (usize, [f32; 4])>) -> Option<usize> {
    let [fx, fy, fw, fh] = from;
    let (fl, ft, fr, fb) = (fx, fy, fx + fw, fy + fh);
    rects.into_iter().filter(|(_, r)| r[2] > 0.0 && r[3] > 0.0).filter_map(|(i, [x, y, w, h])| {
        let (l, t, r, b) = (x, y, x + w, y + h);
        let beyond = match dir {
            Toward::Right => (fl < l || fr <= l) && fr < r,
            Toward::Left => (fr > r || fl >= r) && fl > l,
            Toward::Down => (ft < t || fb <= t) && fb < b,
            Toward::Up => (fb > b || ft >= b) && ft > t,
        };
        if !beyond { return None; }
        let (along, across, shares) = match dir {
            Toward::Right => (l - fr, (t + b - ft - fb) / 2.0, t < fb && b > ft),
            Toward::Left => (fl - r, (t + b - ft - fb) / 2.0, t < fb && b > ft),
            Toward::Down => (t - fb, (l + r - fl - fr) / 2.0, l < fr && r > fl),
            Toward::Up => (ft - b, (l + r - fl - fr) / 2.0, l < fr && r > fl),
        };
        let along = along.max(0.0);
        Some((i, !shares, 13.0 * along * along + across * across))
    }).min_by(|a, b| (a.1, a.2).partial_cmp(&(b.1, b.2)).unwrap_or(std::cmp::Ordering::Equal)).map(|(i, ..)| i)
}

impl<S: 'static> Runtime<S> {
    /// D-pad arrows move focus by position (a television's remote). Off by default: a
    /// keyboard's arrows keep moving within a group and Tab moves between groups.
    pub fn set_spatial_arrows(&mut self, on: bool) {
        self.spatial_arrows = on;
    }

    /// Whether a focused text field keeps its arrows (moving its caret) while arrows are
    /// spatial. On by default. A television host turns it off while a field the remote reached
    /// has no keyboard up, so the remote passes over the field rather than into it, and on
    /// again once the person presses the field to type.
    pub fn set_fields_keep_arrows(&mut self, on: bool) {
        self.fields_keep_arrows = on;
    }

    /// The focusable node an arrow from `from` lands on, or `None` at an edge. Inside a scroll
    /// view the next control in it comes first, even scrolled out of sight (focusing it
    /// scrolls it in), as Android's `ScrollView.arrowScroll` does: otherwise a control drawn
    /// past the list's end, the sidebar's machine beneath its scrolling places, is nearer than
    /// the row the list hides, and the remote skips that row.
    pub(super) fn spatial_target(&self, from: usize, dir: Toward) -> Option<usize> {
        let at = *self.drawn.get(from)?;
        let candidates = || (0..self.nodes.len())
            .filter(move |&j| j != from && self.can_focus(j) && self.nodes[j].focus.order >= 0)
            .filter_map(|j| Some((j, *self.drawn.get(j)?)));
        if let Some(scroll) = self.scroll_around(from) {
            let inside = candidates().filter(|&(j, _)| self.scroll_around(j).is_some_and(|s| self.within(s, scroll)));
            if let Some(next) = nearest(at, dir, inside) { return Some(next); }
        }
        nearest(at, dir, candidates())
    }

    /// The nearest scroll view around node `i`.
    fn scroll_around(&self, i: usize) -> Option<usize> {
        std::iter::successors(self.nodes[i].parent, |&p| self.nodes[p].parent).find(|&p| self.nodes[p].scroll.is_some())
    }

    /// Whether node `i` is `ancestor` or inside it.
    fn within(&self, i: usize, ancestor: usize) -> bool {
        std::iter::successors(Some(i), |&p| self.nodes[p].parent).any(|p| p == ancestor)
    }
}

#[cfg(test)]
#[path = "spatial_tests.rs"]
mod tests;

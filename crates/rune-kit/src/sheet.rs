//! A bottom sheet: what a popover is where the window is touched and narrow (a phone). It
//! rises from the bottom edge over a scrim, as wide as the window, with a grabber at its head;
//! a press on the scrim, Escape (a phone's Back, where the host sends it) or a drag down on
//! the grabber closes it. The surface runs to the window's bottom edge, under a gesture bar,
//! and pads its content clear of it; it never rises into the top inset (a status bar).
//!
//! [`Menu`](crate::Menu) opens as one wherever [`present_menus`] says so, so an app turns
//! every menu it has into a sheet in one place.

use std::cell::Cell;
use std::rc::Rc;

use rune::{Drag, El, Phase, Transition, col, el, row, text};
use rune_motion::Edge;

use crate::overlay::fit_scroll_bar;
use crate::{Kit, shadows};

/// The least height of a row in a sheet, in points: between Material's 48 and 56.
pub const ROW: f32 = 52.0;
/// How far down (points) a drag on the grabber travels before it closes the sheet.
const DISMISS_TRAVEL: f32 = 48.0;
/// The surface's top corners; the bottom ones are past the window's edge.
const RADIUS: f32 = 20.0;

thread_local! {
    static MENUS: Cell<Option<[f32; 4]>> = const { Cell::new(None) };
}

/// Whether this thread's menus open as bottom sheets, and the window's safe area (top, right,
/// bottom, left, in points) they keep clear of. An app says so each frame before it builds its
/// view: `Some` where the window is touched and too narrow for a popover to read, `None` (the
/// default) elsewhere, where a menu is the popover it always was.
pub fn present_menus(insets: Option<[f32; 4]>) {
    MENUS.with(|cell| cell.set(insets));
}

/// The safe area menus on this thread open as sheets inside, if they do ([`present_menus`]).
pub fn menus() -> Option<[f32; 4]> {
    MENUS.with(Cell::get)
}

/// `Sheet::new("share").title("Share").child(rows).open(s.open).on_close(..).build(&kit, &mut layer)`.
pub struct Sheet {
    key: String,
    title: Option<String>,
    label: Option<String>,
    role: &'static str,
    body: Vec<El>,
    open: bool,
    insets: [f32; 4],
    focus_return: Option<String>,
    close: Option<Close>,
}

struct Close {
    scrim: Box<dyn FnOnce(El) -> El>,
    modal: Box<dyn FnOnce(El) -> El>,
    grab: Box<dyn FnOnce(El) -> El>,
}

impl Sheet {
    pub fn new(key: impl Into<String>) -> Sheet {
        Sheet {
            key: key.into(),
            title: None,
            label: None,
            role: "dialog",
            body: Vec::new(),
            open: false,
            insets: menus().unwrap_or([0.0; 4]),
            focus_return: None,
            close: None,
        }
    }
    /// Words at the head of the sheet, and its accessible name.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    /// The accessible name of a sheet that shows no title.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    /// The surface's role: `"dialog"` unless it is a `"menu"`.
    pub fn role(mut self, role: &'static str) -> Self {
        self.role = role;
        self
    }
    pub fn child(mut self, body: El) -> Self {
        self.body.push(body);
        self
    }
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }
    /// The window's safe area (top, right, bottom, left): the default is [`menus`]'s, else none.
    pub fn insets(mut self, insets: [f32; 4]) -> Self {
        self.insets = insets;
        self
    }
    /// The element focus returns to when the sheet closes (its trigger's key).
    pub fn focus_return(mut self, key: impl Into<String>) -> Self {
        self.focus_return = Some(key.into());
        self
    }
    /// A press on the scrim, Escape and a drag down on the grabber close the sheet. A sheet
    /// with no way to close is not built.
    pub fn on_close<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        let (outside, escape, drag) = {
            let f = Rc::new(f);
            (f.clone(), f.clone(), f)
        };
        self.close = Some(Close {
            scrim: Box::new(move |e| e.on_press(move |s: &mut S| outside(s))),
            modal: Box::new(move |e| e.modal(true, move |s: &mut S| escape(s))),
            grab: Box::new(move |e| {
                e.on_drag(move |s: &mut S, d: &mut Drag, _| {
                    if d.phase == Phase::Up && !d.cancelled() && d.dy >= DISMISS_TRAVEL {
                        drag(s);
                    }
                })
            }),
        });
        self
    }

    pub fn build(self, k: &Kit, layer: &mut Vec<El>) {
        if !self.open {
            return;
        }
        let Some(close) = self.close else { return };
        let c = &k.c;
        let [top, right, bottom, left] = self.insets;
        // The grabber: a pill a finger drags down, in a band tall enough to take hold of.
        let grabber = (close.grab)(
            row()
                .key("grabber")
                .role("none")
                .w_full()
                .h(24.0)
                .center()
                .child(el().size(32.0, 4.0).radius(2.0).fill(c.border_base))
                .a11y_hidden(),
        );
        let mut head = col().key("head").w_full().child(grabber);
        if let Some(title) = &self.title {
            head = head.child(
                text(title.clone()).key("title").font(k.ty.ui_medium).color(c.text_base).wrap().px(20.0).pb(8.0).a11y_hidden(),
            );
        }
        let rows = col().key("body").w_full().pl(left).pr(right).children(self.body);
        let name = self.title.or(self.label).unwrap_or_default();
        // The surface is taller than it shows by its radius: the bottom corners sit past the
        // window's edge, so only the top ones are round.
        let mut panel = shadows(
            k,
            col()
                .key(format!("{}-sheet", self.key))
                .role(self.role)
                .label(name)
                .focus_group()
                .w_full()
                .max_w(640.0)
                .self_center()
                .shrink()
                .min_h(0.0)
                .mb(-RADIUS)
                .pb(RADIUS + bottom)
                .radius(RADIUS)
                .fill(c.layer_01)
                .child(head)
                .child(fit_scroll_bar(k, &self.key, rows)),
            &k.elevation.overlay,
        )
        .transition(Transition::move_edge(Edge::Bottom))
        .token_motion(k.motion.overlay);
        if let Some(key) = self.focus_return {
            panel = panel.focus_return(key);
        }
        let panel = (close.modal)(panel);
        layer.push((close.scrim)(
            el().key(format!("{}-scrim", self.key))
                .role("none")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .fill(c.scrim)
                .transition(Transition::opacity())
                .token_motion(k.motion.overlay)
                .a11y_hidden(),
        ));
        layer.push(
            col().key(format!("{}-layer", self.key))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .pt(top + 8.0)
                .justify_end()
                .clip_soft(0.0)
                .child(panel),
        );
    }
}

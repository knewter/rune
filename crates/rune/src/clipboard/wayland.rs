//! Linux's clipboard under Wayland: the window's own connection, through `wl_data_device`, which
//! every compositor offers. arboard reads Wayland only through a data-control protocol (wlroots'
//! or `ext-data-control`); GNOME offers neither, so arboard falls back to X11 there, and finds no
//! clipboard at all where no X display is reachable (a sandbox, a session without Xwayland).
//!
//! Text goes through Wayland. arboard, where it opens, still carries the forms Wayland's text
//! clipboard here does not (HTML, an image, files): its read is used only when its text is the text
//! Wayland holds, so a stale X11 selection is never pasted in place of the real one.

use super::*;
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The `wl_display` of the event loop, once a window has started on Wayland.
static DISPLAY: AtomicUsize = AtomicUsize::new(0);

/// Record the event loop's Wayland display, before the clipboard is first used.
pub(crate) fn set_display(display: *mut c_void) { DISPLAY.store(display as usize, Ordering::Release); }

pub(super) struct Linux {
    wayland: Option<smithay_clipboard::Clipboard>,
    arboard: Option<arboard::Clipboard>,
}

impl Linux {
    pub(super) fn open() -> Option<Self> {
        let display = DISPLAY.load(Ordering::Acquire);
        // SAFETY: the display is the event loop's, which outlives every window and so the
        // clipboard worker (shut down on the event loop's exit path).
        let wayland = (display != 0).then(|| unsafe { smithay_clipboard::Clipboard::new(display as *mut c_void) });
        let arboard = arboard::Clipboard::new().ok();
        (wayland.is_some() || arboard.is_some()).then_some(Self { wayland, arboard })
    }

    fn wayland_text(&self) -> Option<String> { self.wayland.as_ref()?.load().ok() }
}

impl Backend for Linux {
    fn write(&mut self, copy: &ClipboardWrite) -> Result<(), String> {
        match (&self.wayland, &mut self.arboard) {
            // A rich copy goes where its forms can go, and its text through Wayland as well, so a
            // paste here reads it whichever way it asks.
            (Some(wayland), Some(arboard)) if copy.clear_after.is_none() && copy.content.is_rich() => {
                let rich = super::native::write(arboard, copy);
                wayland.store(copy.content.text());
                rich
            }
            (Some(wayland), _) => { wayland.store(copy.content.text()); Ok(()) }
            (None, Some(arboard)) => super::native::write(arboard, copy),
            (None, None) => Err("no clipboard".into()),
        }
    }
    fn text(&mut self) -> Option<Zeroizing<String>> {
        match self.wayland_text() {
            Some(text) => Some(Zeroizing::new(text)),
            None => self.arboard.as_mut()?.text(),
        }
    }
    fn content(&mut self, all: bool) -> Option<ClipboardContent> {
        let text = self.wayland_text();
        let rich = self.arboard.as_mut().and_then(|arboard| arboard.content(all));
        match (text, rich) {
            (Some(text), Some(rich)) if rich.text() == text => Some(rich),
            (Some(text), _) => Some(ClipboardContent::from(text)),
            // Nothing Wayland reads as text (an image alone): what arboard reads, if anything.
            (None, rich) => rich,
        }
    }
    fn revision(&self) -> Option<u64> { super::native::revision() }
    fn clear(&mut self) {
        if let Some(wayland) = &self.wayland { wayland.store(String::new()); }
        if let Some(arboard) = &mut self.arboard { Backend::clear(arboard); }
    }
}

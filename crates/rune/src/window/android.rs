//! Android's window lifecycle. The OS takes the native window away when the app goes to the
//! background (`suspended`) and gives a new one when it comes back (`resumed`). A wgpu surface
//! made on the old window draws nowhere, so the window goes as a Wayland hide does, and comes
//! back through `open`: a new surface on the GPU the host kept, the settled state in its first
//! frame.

use super::*;

impl<S: 'static> Host<S> {
    pub(super) fn suspend_surface(&mut self, event_loop: &ActiveEventLoop) {
        self.suspended_open = self.win.as_ref().is_some_and(|w| w.visible);
        if self.win.is_some() { self.hide(event_loop); }
    }

    pub(super) fn restore_surface(&mut self, event_loop: &ActiveEventLoop) {
        if !std::mem::take(&mut self.suspended_open) || self.finished { return; }
        self.open(event_loop, None);
        self.drain(event_loop, false);
    }
}

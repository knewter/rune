//! Keeping the system's keyboard shortcuts for one Wayland window while it has the keyboard
//! (`Tx::inhibit_shortcuts`), for a remote desktop or a VM's screen that wants Super and
//! Alt+Tab for itself. winit has no such call, so this binds `wl_seat` and
//! `zwp_keyboard_shortcuts_inhibit_manager_v1` on winit's own display connection, in a private
//! queue, and inhibits for winit's surface, as `activation` does for focus.
//!
//! The compositor decides: GNOME asks the user once, and every compositor keeps a key that
//! restores its shortcuts. Dropping the inhibitor gives them back.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1;
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibitor_v1::ZwpKeyboardShortcutsInhibitorV1;
use winit::window::Window;

struct State;

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &WlRegistry, _: <WlRegistry as Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlSeat, ()> for State {
    fn event(_: &mut Self, _: &WlSeat, _: <WlSeat as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwpKeyboardShortcutsInhibitManagerV1, ()> for State {
    fn event(_: &mut Self, _: &ZwpKeyboardShortcutsInhibitManagerV1, _: <ZwpKeyboardShortcutsInhibitManagerV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for State {
    fn event(_: &mut Self, _: &ZwpKeyboardShortcutsInhibitorV1, _: <ZwpKeyboardShortcutsInhibitorV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

/// One window's held shortcuts; dropping it gives them back.
pub(crate) struct Inhibitor {
    queue: EventQueue<State>,
    inhibitor: ZwpKeyboardShortcutsInhibitorV1,
    manager: ZwpKeyboardShortcutsInhibitManagerV1,
    /// The seat the inhibitor names; held for as long as it is.
    _seat: WlSeat,
}

impl Inhibitor {
    /// None off Wayland or when the compositor lacks the protocol.
    pub fn new(window: &Window) -> Option<Inhibitor> {
        let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(handle)) =
            (window.display_handle().ok()?.as_raw(), window.window_handle().ok()?.as_raw())
        else {
            return None;
        };
        // SAFETY: winit owns this `wl_display` and keeps it alive for the event loop, which
        // outlives every window; a foreign backend never closes it.
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        let (globals, queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        let manager = globals.bind::<ZwpKeyboardShortcutsInhibitManagerV1, _, _>(&qh, 1..=1, ()).ok()?;
        let seat = globals.bind::<WlSeat, _, _>(&qh, 1..=1, ()).ok()?;
        // SAFETY: the pointer is the live `wl_surface` proxy winit created for this window, and
        // the inhibitor is dropped before the window.
        let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), handle.surface.as_ptr().cast()) }.ok()?;
        let surface = WlSurface::from_id(&conn, id).ok()?;
        let inhibitor = manager.inhibit_shortcuts(&surface, &seat, &qh, ());
        let _ = queue.flush();
        Some(Inhibitor { queue, inhibitor, manager, _seat: seat })
    }
}

impl Drop for Inhibitor {
    fn drop(&mut self) {
        self.inhibitor.destroy();
        self.manager.destroy();
        let _ = self.queue.flush();
    }
}

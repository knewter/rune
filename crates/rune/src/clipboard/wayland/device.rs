//! The Wayland clipboard on its own event queue: `wl_data_device`, which every compositor offers,
//! in any MIME type. Adapted from smithay-clipboard 0.7.3 (MIT, Copyright (c) 2018 Lucas Timmins &
//! Victor Berger), which carries text alone; here an offer is read in the first of the types a
//! caller prefers, and a copy is offered in every form it holds (text, HTML, PNG, a file list).
//! The primary selection is left out: Rune has none.

use std::collections::HashMap;
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::rc::Rc;
use std::sync::mpsc::Sender;

use sctk::data_device_manager::data_device::{DataDevice, DataDeviceHandler};
use sctk::data_device_manager::data_offer::{DataOfferError, DataOfferHandler, DragOffer};
use sctk::data_device_manager::data_source::{CopyPasteSource, DataSourceHandler};
use sctk::data_device_manager::{DataDeviceManagerState, WritePipe};
use sctk::registry::{ProvidesRegistryState, RegistryState};
use sctk::seat::pointer::{PointerData, PointerEvent, PointerEventKind, PointerHandler};
use sctk::seat::{Capability, SeatHandler, SeatState};
use sctk::{delegate_data_device, delegate_pointer, delegate_registry, delegate_seat, registry_handlers};

use sctk::reexports::calloop::channel::{self, Channel};
use sctk::reexports::calloop::{EventLoop, LoopHandle, PostAction};
use sctk::reexports::calloop_wayland_source::WaylandSource;
use sctk::reexports::client::globals::{GlobalList, registry_queue_init};
use sctk::reexports::client::protocol::wl_data_device::WlDataDevice;
use sctk::reexports::client::protocol::wl_data_device_manager::DndAction;
use sctk::reexports::client::protocol::wl_data_source::WlDataSource;
use sctk::reexports::client::protocol::wl_keyboard::WlKeyboard;
use sctk::reexports::client::protocol::wl_pointer::WlPointer;
use sctk::reexports::client::protocol::wl_seat::WlSeat;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_backend::client::ObjectId;

/// What the clipboard thread is asked.
pub(super) enum Command {
    /// Offer these forms, by MIME type, as the clipboard's selection.
    Store(Vec<(String, Vec<u8>)>),
    /// The MIME types the current selection is offered in.
    Offered,
    /// The selection in the first of these types it is offered in.
    Load(Vec<String>),
    Exit,
}

pub(super) enum Reply {
    Offered(Vec<String>),
    Loaded(Result<(String, Vec<u8>)>),
}

pub(super) fn spawn(connection: Connection, commands: Channel<Command>, replies: Sender<Reply>) -> Option<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name("rune-wayland-clipboard".into())
        .spawn(move || run(connection, commands, replies)).ok()
}

fn run(connection: Connection, commands: Channel<Command>, replies: Sender<Reply>) {
    let Ok((globals, queue)) = registry_queue_init(&connection) else { return };
    let Ok(mut event_loop) = EventLoop::<State>::try_new() else { return };
    let handle = event_loop.handle();
    let Some(mut state) = State::new(&globals, &queue.handle(), handle.clone(), replies) else { return };
    let inserted = handle.insert_source(commands, |event, _, state| {
        let channel::Event::Msg(command) = event else { return };
        match command {
            Command::Store(forms) => { state.store(forms); }
            Command::Offered => { let offered = state.offered(); let _ = state.replies.send(Reply::Offered(offered)); }
            Command::Load(preferred) => {
                if let Err(error) = state.load(&preferred) { let _ = state.replies.send(Reply::Loaded(Err(error))); }
            }
            Command::Exit => state.exit = true,
        }
    });
    if inserted.is_err() || WaylandSource::new(connection, queue).insert(handle).is_err() { return }
    while event_loop.dispatch(None, &mut state).is_ok() && !state.exit {}
}

struct State {
    data_device_manager_state: DataDeviceManagerState,
    replies: Sender<Reply>,
    exit: bool,
    registry_state: RegistryState,
    seat_state: SeatState,
    seats: HashMap<ObjectId, ClipboardSeatState>,
    /// The latest seat which got an event.
    latest_seat: Option<ObjectId>,
    loop_handle: LoopHandle<'static, Self>,
    queue_handle: QueueHandle<Self>,
    data_sources: Vec<CopyPasteSource>,
    /// The forms the selection this client set is offered in.
    forms: Rc<Vec<(String, Rc<[u8]>)>>,
}

impl State {
    fn new(globals: &GlobalList, queue_handle: &QueueHandle<Self>, loop_handle: LoopHandle<'static, Self>, replies: Sender<Reply>) -> Option<Self> {
        let data_device_manager_state = DataDeviceManagerState::bind(globals, queue_handle).ok()?;
        let seat_state = SeatState::new(globals, queue_handle);
        // NOTE: while it's mutable, it's not part of the hash compute.
        #[allow(clippy::mutable_key_type)]
        let seats = seat_state.seats().map(|seat| (seat.id(), ClipboardSeatState::default())).collect();
        Some(Self {
            registry_state: RegistryState::new(globals),
            queue_handle: queue_handle.clone(),
            data_device_manager_state, data_sources: Vec::new(), latest_seat: None,
            loop_handle, exit: false, seat_state, replies, seats, forms: Rc::new(Vec::new()),
        })
    }

    fn focused_seat(&self) -> Result<&ClipboardSeatState> {
        let latest = self.latest_seat.as_ref().ok_or_else(|| Error::other("no events received on any seat"))?;
        let seat = self.seats.get(latest).ok_or_else(|| Error::other("active seat lost"))?;
        if !seat.has_focus { return Err(Error::other("client doesn't have focus")); }
        Ok(seat)
    }

    fn store(&mut self, forms: Vec<(String, Vec<u8>)>) -> Option<()> {
        let seat = self.focused_seat().ok()?;
        let device = seat.data_device.as_ref()?;
        let serial = seat.latest_serial;
        let source = self.data_device_manager_state.create_copy_paste_source(&self.queue_handle, forms.iter().map(|(mime, _)| mime.as_str()));
        source.set_selection(device, serial);
        self.forms = Rc::new(forms.into_iter().map(|(mime, bytes)| (mime, Rc::from(bytes))).collect());
        self.data_sources.push(source);
        Some(())
    }

    fn offered(&self) -> Vec<String> {
        self.focused_seat().ok()
            .and_then(|seat| seat.data_device.as_ref()?.data().selection_offer())
            .map(|offer| offer.with_mime_types(|types| types.to_vec()))
            .unwrap_or_default()
    }

    /// Read the selection in the first preferred type it is offered in; the reply is sent when
    /// the pipe closes.
    fn load(&mut self, preferred: &[String]) -> Result<()> {
        let seat = self.focused_seat()?;
        let offer = seat.data_device.as_ref().and_then(|data| data.data().selection_offer())
            .ok_or_else(|| Error::other("selection is empty"))?;
        let mime = offer.with_mime_types(|offered| preferred.iter().find(|want| offered.contains(want)).cloned())
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "supported mime-type is not found"))?;
        let pipe = offer.receive(mime.clone()).map_err(|error| match error {
            DataOfferError::InvalidReceive => Error::other("offer is not ready yet"),
            DataOfferError::Io(error) => error,
        })?;
        set_non_blocking(pipe.as_raw_fd())?;
        let mut buffer = [0; 16384];
        let mut content = Vec::new();
        let _ = self.loop_handle.insert_source(pipe, move |_, file, state| {
            // SAFETY: the pipe is this source's alone until it is removed.
            let file = unsafe { file.get_mut() };
            loop {
                match file.read(&mut buffer) {
                    Ok(0) => {
                        let _ = state.replies.send(Reply::Loaded(Ok((mime.clone(), std::mem::take(&mut content)))));
                        break PostAction::Remove;
                    }
                    Ok(n) if content.len() + n > super::MAX_READ => {
                        let _ = state.replies.send(Reply::Loaded(Err(Error::other("clipboard content is too large"))));
                        break PostAction::Remove;
                    }
                    Ok(n) => content.extend_from_slice(&buffer[..n]),
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break PostAction::Continue,
                    Err(error) => {
                        let _ = state.replies.send(Reply::Loaded(Err(error)));
                        break PostAction::Remove;
                    }
                }
            }
        });
        Ok(())
    }

    fn send_request(&mut self, write_pipe: WritePipe, mime: String) {
        let Some(contents) = self.forms.iter().find(|(offered, _)| *offered == mime).map(|(_, bytes)| bytes.clone()) else { return };
        if set_non_blocking(write_pipe.as_raw_fd()).is_err() { return; }
        let mut written = 0;
        let _ = self.loop_handle.insert_source(write_pipe, move |_, file, _| {
            // SAFETY: the pipe is this source's alone until it is removed.
            let file = unsafe { file.get_mut() };
            loop {
                if written == contents.len() { break PostAction::Remove; }
                match file.write(&contents[written..]) {
                    Ok(n) => written += n,
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break PostAction::Continue,
                    Err(_) => break PostAction::Remove,
                }
            }
        });
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState { &mut self.seat_state }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: WlSeat) { self.seats.insert(seat.id(), Default::default()); }
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: WlSeat, capability: Capability) {
        let Some(seat_state) = self.seats.get_mut(&seat.id()) else { return };
        match capability {
            Capability::Keyboard => {
                seat_state.keyboard = Some(seat.get_keyboard(qh, seat.id()));
                // Selection sources are tied to the keyboard.
                if seat_state.data_device.is_none() {
                    seat_state.data_device = Some(self.data_device_manager_state.get_data_device(qh, &seat));
                }
            }
            Capability::Pointer => seat_state.pointer = self.seat_state.get_pointer(qh, &seat).ok(),
            _ => (),
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: WlSeat, capability: Capability) {
        let Some(seat_state) = self.seats.get_mut(&seat.id()) else { return };
        match capability {
            Capability::Keyboard => {
                seat_state.data_device = None;
                if let Some(keyboard) = seat_state.keyboard.take() && keyboard.version() >= 3 { keyboard.release() }
            }
            Capability::Pointer => {
                if let Some(pointer) = seat_state.pointer.take() && pointer.version() >= 3 { pointer.release() }
            }
            _ => (),
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: WlSeat) { self.seats.remove(&seat.id()); }
}

impl PointerHandler for State {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, pointer: &WlPointer, events: &[PointerEvent]) {
        let Some(seat) = pointer.data::<PointerData>().map(|data| data.seat().id()) else { return };
        let Some(seat_state) = self.seats.get_mut(&seat) else { return };
        let mut updated = false;
        for event in events {
            if let PointerEventKind::Press { serial, .. } | PointerEventKind::Release { serial, .. } = event.kind {
                updated = true;
                seat_state.latest_serial = serial;
            }
        }
        // Only update the seat in use when the serial moved.
        if updated { self.latest_seat = Some(seat); }
    }
}

impl DataDeviceHandler for State {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64, _: &WlSurface) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    // The selection is finished and ready to be used.
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
}

impl DataSourceHandler for State {
    fn send_request(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, mime: String, write_pipe: WritePipe) {
        self.send_request(write_pipe, mime)
    }
    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, deleted: &WlDataSource) {
        self.data_sources.retain(|source| source.inner() != deleted)
    }
    fn accept_mime(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: Option<String>) {}
    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
}

impl DataOfferHandler for State {
    fn source_actions(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
    fn selected_action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
}

impl ProvidesRegistryState for State {
    registry_handlers![SeatState];
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry_state }
}

impl Dispatch<WlKeyboard, ObjectId, State> for State {
    fn event(state: &mut State, _: &WlKeyboard, event: <WlKeyboard as Proxy>::Event, data: &ObjectId, _: &Connection, _: &QueueHandle<State>) {
        use sctk::reexports::client::protocol::wl_keyboard::Event as WlKeyboardEvent;
        let Some(seat_state) = state.seats.get_mut(data) else { return };
        match event {
            WlKeyboardEvent::Key { serial, .. } | WlKeyboardEvent::Modifiers { serial, .. } => {
                seat_state.latest_serial = serial;
                state.latest_seat = Some(data.clone());
            }
            // The selection relies on keyboard focus.
            WlKeyboardEvent::Enter { serial, .. } => {
                seat_state.latest_serial = serial;
                seat_state.has_focus = true;
                state.latest_seat.get_or_insert_with(|| data.clone());
            }
            WlKeyboardEvent::Leave { .. } => {
                seat_state.latest_serial = 0;
                seat_state.has_focus = false;
            }
            _ => (),
        }
    }
}

delegate_seat!(State);
delegate_pointer!(State);
delegate_data_device!(State);
delegate_registry!(State);

#[derive(Debug, Default)]
struct ClipboardSeatState {
    keyboard: Option<WlKeyboard>,
    pointer: Option<WlPointer>,
    data_device: Option<DataDevice>,
    has_focus: bool,
    /// The latest serial used to set the selection content.
    latest_serial: u32,
}

impl Drop for ClipboardSeatState {
    fn drop(&mut self) {
        if let Some(keyboard) = self.keyboard.take() && keyboard.version() >= 3 { keyboard.release(); }
        if let Some(pointer) = self.pointer.take() && pointer.version() >= 3 { pointer.release(); }
    }
}

fn set_non_blocking(raw_fd: RawFd) -> Result<()> {
    // SAFETY: fcntl on a descriptor this module owns.
    let flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFL) };
    if flags < 0 { return Err(Error::last_os_error()); }
    // SAFETY: as above.
    if unsafe { libc::fcntl(raw_fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 { return Err(Error::last_os_error()); }
    Ok(())
}

//! Pacing for frames no display tick drives: timer deadlines, a hidden host's refresh pace,
//! and what follows a frame whose drawable never came.

use super::*;

/// The runtime's next timer, on the wall clock that started with the host.
pub(crate) fn wall_wake_deadline<S>(rt: &Runtime<S>) -> Option<Instant> {
    rt.wall_start?.checked_add(Duration::try_from_secs_f64(rt.wake_deadline()?).ok()?)
}

/// The runtime's next timer as a host with no display link to take it waits for it: hidden,
/// covered, faded out or unpaced. A timer due within a frame interval of the last poll
/// (`Tx::wake_after(Duration::ZERO)` asks for the next frame) waits for that interval, so a
/// poll that keeps asking steps at most once per `TIMER_FRAME` and never spins the loop.
pub(crate) fn timer_deadline<S>(rt: &Runtime<S>) -> Option<Instant> {
    let due = wall_wake_deadline(rt)?;
    let polled = rt.wall_start.and_then(|t0| t0.checked_add(Duration::try_from_secs_f64(rt.polled_at()).ok()?));
    Some(polled.map_or(due, |polled| due.max(polled + TIMER_FRAME)))
}

/// Paces the frame a hidden host keeps ready to show (a launcher's cached pixels, a retained
/// overlay's commit). A change after a quiet spell renders at once; later ones wait until
/// `HIDDEN_REFRESH` has passed since the last render, so state that changes many times a
/// second costs a few builds a second while nobody sees them.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HiddenPace { last: Option<Instant>, waiting: bool }

pub(crate) const HIDDEN_REFRESH: Duration = Duration::from_millis(250);

impl HiddenPace {
    /// Whether a change at `now` renders now. Otherwise it waits for `deadline`.
    pub(crate) fn changed(&mut self, now: Instant) -> bool {
        self.waiting = !self.ready(now);
        !self.waiting
    }
    /// Whether a change at `now` would render at once.
    pub(crate) fn ready(&self, now: Instant) -> bool {
        self.last.is_none_or(|last| now >= last + HIDDEN_REFRESH)
    }
    /// When the change that waits renders.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.last.filter(|_| self.waiting).map(|last| last + HIDDEN_REFRESH)
    }
    /// Whether the change that waits is due at `now`; it stops waiting either way it renders.
    pub(crate) fn take_due(&mut self, now: Instant) -> bool {
        let due = self.deadline().is_some_and(|at| at <= now);
        if due { self.waiting = false; }
        due
    }
    /// A hidden frame rendered what had changed.
    pub(crate) fn rendered(&mut self, now: Instant) {
        *self = HiddenPace { last: Some(now), waiting: false };
    }
    /// Shown, the host draws its own frames; hidden again, the first change renders at once.
    pub(crate) fn reset(&mut self) { *self = HiddenPace::default(); }
}

/// A frame a due timer asked a shown window for, which hasn't come. A compositor may hold it
/// back for as long as it likes: Wayland's winit delivers `RedrawRequested` only once the last
/// frame callback arrives, and a compositor sends none to a surface nobody sees (another
/// workspace, minimized, wholly covered), and says nothing about why. The timer that asked
/// stays due all that while, so it may not set the next wake: `deadline` holds it back until
/// `wait` after the ask. A wait that runs out found the frame withheld; the host then runs
/// the timer covered (`covered_step`), so its poll and inbox keep moving, and the wait
/// doubles to `OCCLUDED_MAX`. Without it, an idle window on another workspace turned the
/// event loop as fast as it could for as long as it stayed there (76% of a core, 21 hours).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AskedFrame { at: Option<Instant>, wait: Duration }

impl AskedFrame {
    /// A due timer asked for a frame at `now`. An ask still outstanding keeps its time.
    pub(crate) fn ask(&mut self, now: Instant) {
        if self.at.is_none() { *self = AskedFrame { at: Some(now), wait: TIMER_FRAME }; }
    }
    /// The frame came (or the window drew for any other reason): timers set the wake again.
    pub(crate) fn drawn(&mut self) { *self = AskedFrame::default(); }
    /// Whether a frame asked for hasn't come yet.
    pub(crate) fn outstanding(&self) -> bool { self.at.is_some() }
    /// The earliest a due timer may wake the loop while the frame it asked for is outstanding.
    pub(crate) fn deadline(&self, due: Instant) -> Instant {
        self.at.map_or(due, |at| due.max(at + self.wait))
    }
    /// At a due timer: whether the frame asked for has been withheld its whole wait. If so, the
    /// next wait starts now and is twice as long, from `OCCLUDED_RETRY` up to `OCCLUDED_MAX`.
    pub(crate) fn withheld(&mut self, now: Instant) -> bool {
        let Some(at) = self.at else { return false };
        if now < at + self.wait { return false; }
        self.at = Some(now);
        self.wait = (self.wait * 2).clamp(OCCLUDED_RETRY, OCCLUDED_MAX);
        true
    }
}

/// Whether a timer asks for the frame after one just presented. Only a surface whose display
/// link paces it (`next_frame` is its next tick) takes a timer due by then as a frame request;
/// anywhere else the timer waits in `timer_deadline`, so it never turns the loop by itself.
pub(crate) fn timer_wants_frame<S>(rt: &Runtime<S>, next_frame: Option<Instant>) -> bool {
    next_frame.is_some_and(|next| wall_wake_deadline(rt).is_some_and(|due| due <= next))
}

/// Why a surface gave no drawable this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Missed {
    /// Nothing of the window is on screen (covered, or its displays asleep).
    Occluded,
    /// No drawable was free, or acquisition failed some other passing way.
    Timeout,
    /// Outdated or lost, and reconfiguring didn't bring it back.
    Outdated,
    /// The surface refused its configuration. wgpu panics on its next drawable, so its owner
    /// must drop it.
    Unconfigured,
}

/// The first re-check of an occluded surface. Its occlusion notification usually redraws it
/// first; this catches one that never arrives. Consecutive misses back off to `OCCLUDED_MAX`.
pub(crate) const OCCLUDED_RETRY: Duration = Duration::from_millis(250);
pub(crate) const OCCLUDED_MAX: Duration = Duration::from_secs(2);
/// An unpaced surface's retry after a drawable timeout: about one frame.
pub(crate) const TIMEOUT_RETRY: Duration = Duration::from_millis(16);
/// Motion's frame interval when no display link paces a surface.
pub(crate) const TIMER_FRAME: Duration = Duration::from_micros(16_667);

impl Missed {
    pub(super) fn of(status: wgpu::CurrentSurfaceTexture) -> Result<wgpu::SurfaceTexture, Missed> {
        use wgpu::CurrentSurfaceTexture as S;
        match status {
            S::Success(f) | S::Suboptimal(f) => Ok(f),
            S::Occluded => Err(Missed::Occluded),
            S::Outdated | S::Lost => Err(Missed::Outdated),
            S::Timeout | S::Validation => Err(Missed::Timeout),
        }
    }

    /// When to try again after a frame this surface couldn't draw, or `None` to sleep until an
    /// event (a display tick, a notification, input or a timer). There is no `ControlFlow::Poll`
    /// here: retrying at once is what spins a core while a window is occluded or its displays
    /// sleep. A display link that is still running retries a timeout on its next tick.
    /// `occluded_wait` is the caller's backoff, held to `OCCLUDED_RETRY..=OCCLUDED_MAX`; an
    /// occluded miss waits it out, then doubles it.
    pub(crate) fn retry(self, paced: bool, now: Instant, occluded_wait: &mut Duration) -> Option<Instant> {
        match self {
            Missed::Occluded => {
                let at = now + (*occluded_wait).clamp(OCCLUDED_RETRY, OCCLUDED_MAX);
                *occluded_wait = (*occluded_wait * 2).min(OCCLUDED_MAX);
                Some(at)
            }
            Missed::Timeout if paced => None,
            Missed::Timeout => Some(now + TIMEOUT_RETRY),
            Missed::Outdated | Missed::Unconfigured => Some(now + OCCLUDED_RETRY),
        }
    }
}

/// The next drawable. An outdated or lost surface is reconfigured and tried again, up to
/// `reconfigures` times, synchronously; every other miss returns at once. On macOS a surface
/// wgpu would call occluded is never asked: each failed `get_current_texture` leaks a texture
/// id in wgpu-core 30.0.1.
pub(crate) fn acquire(surface: &wgpu::Surface, device: &wgpu::Device, config: &wgpu::SurfaceConfiguration, reconfigures: u32,
    meter: &mut SurfaceMeter) -> Result<wgpu::SurfaceTexture, Missed> {
    acquire_checked(surface, device, config, reconfigures, meter, true)
}

/// `acquire`, skipping the occlusion check when the caller has just read the window hosting
/// the surface as visible (`check_occlusion` false).
pub(crate) fn acquire_checked(surface: &wgpu::Surface, device: &wgpu::Device, config: &wgpu::SurfaceConfiguration, mut reconfigures: u32,
    meter: &mut SurfaceMeter, check_occlusion: bool) -> Result<wgpu::SurfaceTexture, Missed> {
    #[cfg(not(target_os = "macos"))]
    let _ = check_occlusion;
    #[cfg(target_os = "macos")]
    if check_occlusion && crate::platform::macos::surface_occluded(surface) {
        meter.skipped();
        return Err(Missed::Occluded);
    }
    loop {
        let status = Missed::of(surface.get_current_texture());
        if let Err(missed) = status { meter.failed(missed); }
        match status {
            // A surface that won't take its configuration again is reported rather than
            // panicking in wgpu's default error handler.
            Err(Missed::Outdated) if reconfigures > 0 => {
                if let Err(error) = super::configure(surface, device, config) {
                    eprintln!("rune: a surface refused its configuration: {error}");
                    return Err(Missed::Unconfigured);
                }
                reconfigures -= 1;
            }
            Err(Missed::Outdated) => {
                return Err(match super::configure(surface, device, config) {
                    Ok(()) => Missed::Outdated,
                    Err(error) => {
                        eprintln!("rune: a surface refused its configuration: {error}");
                        Missed::Unconfigured
                    }
                });
            }
            other => return other,
        }
    }
}

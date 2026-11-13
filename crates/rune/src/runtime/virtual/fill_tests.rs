//! The fill machine against origin/dev's counter, and the wait that resumes a starved fill.
use super::{Demand, FILL_FRAMES, Fill, FillEffect, FillEvent, RESUME_LIMIT, Sources, State};
use crate::machine::{Attempt, Effects};

/// The lists one source shows, and the lists after another source replaced it.
const A: Sources = Sources(1);
const B: Sources = Sources(2);

/// origin/dev's `Runtime::virtual_filling`, transcribed as it was: `Runtime.virtual_fill: u32`
/// counted one more for each frame that found an index-built list wanting its window and reset
/// to zero for each frame that found none, and it answered `filling && virtual_fill <= 12` —
/// `VIRTUAL_FILL_FRAMES` at `runtime/virtual/window.rs:8`. It is never edited to fit the branch:
/// a walk that fails means the branch differs from dev.
struct Dev {
    virtual_fill: u32,
}

impl Dev {
    fn frame(&mut self, filling: bool) -> bool {
        self.virtual_fill = if filling { self.virtual_fill + 1 } else { 0 };
        filling && self.virtual_fill <= 12
    }
}

fn sequence(bits: u32, len: usize) -> String {
    (0..len).map(|i| if bits >> i & 1 == 1 { 'F' } else { '.' }).collect()
}

/// Runs `fill` to the bound with `missing` rows unfilled, returning the wait it asked for.
fn run_to_bound(fill: &mut Fill, now: f64, missing: usize) -> Option<(f64, Attempt)> {
    for _ in 0..FILL_FRAMES + 2 {
        let count = if fill.at_bound() { missing } else { 0 };
        for effect in fill.step(FillEvent::Frame { now, wanted: true, missing: count, moved: false, sources: A }) {
            if let FillEffect::WakeAt(due) = effect
                && let State::Waiting { attempt, .. } = fill.state()
            {
                return Some((due, attempt));
            }
        }
        if matches!(fill.state(), State::Idle | State::Stalled { .. }) { return None; }
    }
    None
}

/// One frame as `Runtime::virtual_filling` runs it, through [`Fill::frame`]: the fill's own
/// deadline draws for itself, a stale one draws nothing, and any other frame counts as a fill
/// frame. Returns whether it drew.
fn frame(fill: &mut Fill, now: f64, missing: usize, deadlines: &mut Vec<f64>) -> bool {
    let effects = fill.frame(now, Demand { wanted: true, moved: false, sources: A }, || missing);
    let drew = effects.clone().into_iter().any(|e| e == FillEffect::Draw);
    for effect in effects {
        if let FillEffect::WakeAt(due) = effect { deadlines.push(due); }
    }
    drew
}

/// Every sequence of frames up to two past the bound, each driven through `step` and its answer
/// compared with dev's after every frame. The wake is not a frame's answer, so the draw answers
/// still match dev exactly: past the bound both stop drawing until a frame finds nothing wanted.
#[test]
fn every_run_of_frames_draws_what_dev_draws() {
    let longest = FILL_FRAMES as usize + 2;
    for len in 1..=longest {
        for bits in 0..1u32 << len {
            let walk = sequence(bits, len);
            let (mut fill, mut dev) = (Fill::default(), Dev { virtual_fill: 0 });
            for (i, wanted) in (0..len).map(|i| (i, bits >> i & 1 == 1)) {
                let missing = if fill.at_bound() { 1 } else { 0 };
                let drew = fill.step(FillEvent::Frame { now: 0.0, wanted, missing, moved: false, sources: A }).into_iter().any(|e| e == FillEffect::Draw);
                assert_eq!(
                    drew,
                    dev.frame(wanted),
                    "frame {i} of {walk}: wanted {wanted}, state {:?}",
                    fill.state()
                );
            }
        }
    }
}

/// One frame's answer is all the runtime keeps of the fill, so a settled list asks again at once,
/// while a run keeps asking until the bound and not after it.
#[test]
fn a_settled_frame_forgets_the_run_and_the_bound_stops_it() {
    let mut fill = Fill::default();
    let frame = |fill: &mut Fill, wanted: bool| {
        let missing = if fill.at_bound() { 8 } else { 0 };
        fill.step(FillEvent::Frame { now: 0.0, wanted, missing, moved: false, sources: A }).into_iter().any(|e| e == FillEffect::Draw)
    };
    let drew: Vec<bool> = (0..FILL_FRAMES as usize + 1).map(|_| frame(&mut fill, true)).collect();
    assert!(drew[..FILL_FRAMES as usize].iter().all(|d| *d), "the first {FILL_FRAMES} frames draw: {drew:?}");
    assert!(!drew[FILL_FRAMES as usize], "the bound stops drawing: {drew:?}");
    // // differs from dev: the bound leaves a wait behind, not a spent count.
    assert!(matches!(fill.state(), State::Waiting { .. }), "the bound waits: {:?}", fill.state());
    assert!(fill.step(FillEvent::Frame { now: 0.0, wanted: false, missing: 0, moved: false, sources: A }).is_empty());
    assert_eq!(fill.state(), State::Idle, "a frame with nothing to fill settles it");
    assert!(frame(&mut fill, true), "a settled list asks again at once");
}

/// A fill that hits its bound with rows still unfilled waits, and only the wake it holds resumes
/// it. A stale attempt, a pre-due frame and a redrawn frame all change nothing.
#[test]
fn the_bound_waits_and_the_wake_resumes_the_fill() {
    let mut fill = Fill::default();
    let (due, attempt) = run_to_bound(&mut fill, 1.0, 4).expect("the bound asks for a wake");
    let waiting = fill.state();
    assert!(matches!(waiting, State::Waiting { .. }));
    assert!(!fill.resuming(due - 0.001), "nothing resumes before the wake is due");
    assert!(fill.resuming(due));

    // A stale wake: an attempt the state never issued changes nothing.
    assert!(fill.step(FillEvent::Due { attempt: Attempt(999), now: 2.0, sources: A }).is_empty());
    assert_eq!(fill.state(), waiting);

    // A frame drawn for another cause while the fill waits: no draw, no reset.
    assert!(fill.step(FillEvent::Frame { now: 1.5, wanted: true, missing: 4, moved: false, sources: A }).is_empty());
    assert_eq!(fill.state(), waiting, "a redraw changes nothing");

    // Its own wake resumes the run it was waiting in.
    let effects = fill.step(FillEvent::Due { attempt, now: due, sources: A });
    assert!(effects.into_iter().any(|e| e == FillEffect::Draw), "the wake draws again");
    assert!(matches!(fill.state(), State::Filling { frames: 1, resumes: 1, .. }), "{:?}", fill.state());
}

/// A list whose heights never settle resumes with a longer wait each time, then stalls: nothing
/// after that asks for a frame, whatever else draws.
#[test]
fn a_starved_fill_stops_after_its_resumes_without_progress() {
    let mut fill = Fill::default();
    let mut now = 0.0;
    let mut waits = Vec::new();
    while let Some((due, attempt)) = run_to_bound(&mut fill, now, 8) {
        waits.push(due - now);
        now = due;
        assert!(waits.len() <= RESUME_LIMIT as usize, "resumes are bounded: {waits:?}");
        assert!(!fill.step(FillEvent::Due { attempt, now, sources: A }).is_empty(), "the wake resumes it");
    }
    assert_eq!(waits.len(), RESUME_LIMIT as usize, "resumes are bounded: {waits:?}");
    assert!(waits.windows(2).all(|w| w[1] > w[0]), "each resume backs off: {waits:?}");
    assert_eq!(fill.state(), State::Stalled { sources: A }, "and the fill stops asking");
    assert!(!fill.resuming(now + 100.0), "nothing is left armed");
    // Redraws and stale deadlines after it change nothing.
    for _ in 0..5 {
        assert!(fill.step(FillEvent::Frame { now: now + 1.0, wanted: true, missing: 8, moved: false, sources: A }).is_empty());
        assert!(fill.step(FillEvent::Due { attempt: Attempt(999), now: now + 1.0, sources: A }).is_empty());
    }
    assert_eq!(fill.state(), State::Stalled { sources: A });
}

/// The reviewer's replay, 36,001 ticks at 359.94 Hz: wanted true with the same eight rows missing
/// forever, one frame per draw and one per deadline, and extra deadlines left in the set. The
/// fill must stop drawing within its bound and leave nothing armed.
#[test]
fn stale_deadlines_never_restart_the_fill() {
    const TICKS: usize = 36_001;
    let dt = 1.0 / 359.94;
    let mut fill = Fill::default();
    // Deadlines nobody's attempt stands for, as a retired wait leaves behind.
    let mut deadlines: Vec<f64> = (0..20).map(|i| dt * f64::from(i * 7 + 3)).collect();
    let (mut now, mut draw, mut frames, mut drawn) = (0.0, true, 0, 0);
    for _ in 0..TICKS {
        now += dt;
        let due = deadlines.iter().any(|d| *d <= now);
        if !draw && !due { continue; }
        frames += 1;
        deadlines.retain(|d| *d > now);
        draw = frame(&mut fill, now, 8, &mut deadlines);
        if draw { drawn += 1; }
    }
    assert!(drawn <= 4 * FILL_FRAMES as usize + 4, "the fill stopped drawing: {drawn} frames of {frames}");
    assert_eq!(fill.state(), State::Stalled { sources: A }, "and it stays stalled");
    assert!(deadlines.is_empty(), "no deadline is left armed: {deadlines:?}");
}

/// r367: a due wake after the list finished or unmounted draws nothing, and starts nothing.
#[test]
fn a_wake_after_the_list_is_gone_draws_nothing() {
    let mut fill = Fill::default();
    let (due, attempt) = run_to_bound(&mut fill, 0.0, 4).expect("the bound asks for a wake");
    assert!(matches!(fill.state(), State::Waiting { .. }));

    // The list finishes or unmounts: the wait retires with it, and its deadline is forgotten.
    assert!(fill.step(FillEvent::Frame { now: 0.1, wanted: false, missing: 0, moved: false, sources: A }).is_empty());
    assert_eq!(fill.state(), State::Idle);
    assert!(!fill.resuming(due), "a retired deadline resumes nothing");
    assert_eq!(fill.wake(due), None, "the deadline it left is no longer the fill's");
    assert!(fill.step(FillEvent::Due { attempt, now: due, sources: A }).is_empty(), "and draws nothing");
    assert_eq!(fill.state(), State::Idle);
    // A list that replaces it starts exactly as one that was never here: its first frame draws.
    assert!(fill.step(FillEvent::Frame { now: due + 1.0, wanted: true, missing: 8, moved: false, sources: A })
        .into_iter().any(|e| e == FillEffect::Draw));
}

/// A resume that finds the unfilled range smaller counts as progress and resets the bound, so a
/// slow fill that is still shrinking keeps going.
#[test]
fn a_resume_that_finds_progress_gets_a_fresh_bound() {
    let mut fill = Fill::default();
    let mut now = 0.0;
    let mut missing = 40;
    let mut wakes = 0;
    while let Some((due, attempt)) = run_to_bound(&mut fill, now, missing) {
        wakes += 1;
        now = due;
        missing = missing.saturating_sub(6);
        assert!(!fill.step(FillEvent::Due { attempt, now, sources: A }).is_empty());
        assert!(wakes <= 8, "progress keeps it going: {wakes}");
    }
    assert!(wakes > RESUME_LIMIT as usize, "the shrinking range earned more resumes: {wakes}");
}

/// A viewport that moved invalidates the window the fill was chosen for, so even a stalled fill
/// starts again; a redraw alone does not.
#[test]
fn a_moved_viewport_starts_the_fill_again() {
    let mut fill = Fill::default();
    let mut now = 0.0;
    while let Some((due, attempt)) = run_to_bound(&mut fill, now, 8) {
        now = due;
        fill.step(FillEvent::Due { attempt, now, sources: A });
    }
    assert_eq!(fill.state(), State::Stalled { sources: A });
    assert!(fill.step(FillEvent::Frame { now: now + 1.0, wanted: true, missing: 8, moved: false, sources: A }).is_empty());
    assert_eq!(fill.state(), State::Stalled { sources: A }, "a redraw leaves it stalled");
    let effects: Effects<FillEffect> = fill.step(FillEvent::Frame { now: now + 2.0, wanted: true, missing: 8, moved: true, sources: A });
    assert!(effects.into_iter().any(|e| e == FillEffect::Draw), "a scroll starts it again");
    assert!(matches!(fill.state(), State::Filling { frames: 1, resumes: 0, .. }), "{:?}", fill.state());
}

/// r374: a list in another's place, or showing another source, starts its own run whatever the
/// old run was doing: filling, waiting or stalled.
#[test]
fn another_source_starts_its_own_run() {
    let mut fill = Fill::default();
    for _ in 0..3 {
        fill.step(FillEvent::Frame { now: 0.0, wanted: true, missing: 0, moved: false, sources: A });
    }
    assert!(matches!(fill.state(), State::Filling { frames: 3, sources: A, .. }), "{:?}", fill.state());
    let effects = fill.step(FillEvent::Frame { now: 0.0, wanted: true, missing: 0, moved: false, sources: B });
    assert!(effects.into_iter().any(|e| e == FillEffect::Draw));
    assert!(matches!(fill.state(), State::Filling { frames: 1, resumes: 0, sources: B, .. }), "filling: {:?}", fill.state());

    let mut fill = Fill::default();
    let (due, _) = run_to_bound(&mut fill, 0.0, 8).expect("the bound asks for a wake");
    let effects = fill.step(FillEvent::Frame { now: 0.01, wanted: true, missing: 8, moved: false, sources: B });
    assert!(effects.into_iter().any(|e| e == FillEffect::Draw), "waiting: the new source draws at once");
    assert!(matches!(fill.state(), State::Filling { frames: 1, resumes: 0, best: usize::MAX, sources: B }), "{:?}", fill.state());
    assert!(!fill.resuming(due), "and the old source's deadline is gone");

    let mut fill = Fill::default();
    let mut now = 0.0;
    while let Some((due, attempt)) = run_to_bound(&mut fill, now, 8) {
        now = due;
        fill.step(FillEvent::Due { attempt, now, sources: A });
    }
    assert_eq!(fill.state(), State::Stalled { sources: A });
    let effects = fill.step(FillEvent::Frame { now: now + 1.0, wanted: true, missing: 8, moved: false, sources: B });
    assert!(effects.into_iter().any(|e| e == FillEffect::Draw), "stalled: the new source draws at once");
    assert!(matches!(fill.state(), State::Filling { frames: 1, resumes: 0, sources: B, .. }), "{:?}", fill.state());
}

/// r374: the old source's deadline comes due in the frame that finds the new source. That frame
/// is the new run's first, not a resume of the old run.
#[test]
fn the_old_deadline_in_a_new_source_frame_starts_its_own_run() {
    let mut fill = Fill::default();
    let (due, _) = run_to_bound(&mut fill, 0.0, 8).expect("the bound asks for a wake");
    let effects = fill.frame(due, Demand { wanted: true, moved: false, sources: B }, || unreachable!("no bound reached"));
    assert!(effects.into_iter().any(|e| e == FillEffect::Draw));
    assert!(matches!(fill.state(), State::Filling { frames: 1, resumes: 0, best: usize::MAX, sources: B }), "{:?}", fill.state());
}

/// origin/dev's fill (`runtime/virtual/fill.rs`) and the half of `Runtime::virtual_filling`
/// after it read the lists (`runtime/virtual/window.rs`), transcribed as they were, with plain
/// numbers for attempts and its constants written out. It is never edited to fit the branch:
/// a walk that fails means the branch differs from dev. Each `// differs from dev:` marks an
/// approved change.
mod reference {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(super) enum State {
        Idle,
        Filling { frames: u16, best: usize, resumes: u8 },
        Waiting { attempt: u32, due: f64, best: usize, resumes: u8 },
        Stalled,
    }

    #[derive(Clone, Copy)]
    enum Event {
        Frame { now: f64, wanted: bool, missing: usize, moved: bool, sources: u64 },
        Due { attempt: u32, now: f64, sources: u64 },
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Effect {
        Draw,
        WakeAt(f64),
    }

    pub(super) struct Dev {
        pub(super) state: State,
        seq: u32,
        pub(super) wake: Option<(u32, f64)>,
        // differs from dev: the sources the run started for.
        pub(super) sources: u64,
    }

    impl Dev {
        pub(super) fn new() -> Self {
            Dev { state: State::Idle, seq: 0, wake: None, sources: 0 }
        }

        fn at_bound(&self) -> bool {
            matches!(self.state, State::Filling { frames, .. } if frames >= 12)
        }

        fn wake(&mut self, now: f64) -> Option<u32> {
            let (attempt, due) = self.wake?;
            if now < due { return None; }
            self.wake = None;
            Some(attempt)
        }

        fn step(&mut self, event: Event) -> Vec<Effect> {
            let effects = self.transition(event);
            if !matches!(self.state, State::Waiting { .. }) { self.wake = None; }
            effects
        }

        fn transition(&mut self, event: Event) -> Vec<Effect> {
            match (self.state, event) {
                (_, Event::Frame { wanted: false, .. }) => {
                    self.state = State::Idle;
                    vec![]
                }
                (_, Event::Frame { wanted: true, moved: true, sources, .. }) => {
                    self.state = State::Filling { frames: 1, best: usize::MAX, resumes: 0 };
                    // differs from dev: the run notes the sources it started for.
                    self.sources = sources;
                    vec![Effect::Draw]
                }
                // differs from dev: a frame for other sources than the run's starts a new run,
                // whatever the old one was doing.
                (state, Event::Frame { wanted: true, sources, .. }) if state != State::Idle && sources != self.sources => {
                    self.state = State::Filling { frames: 1, best: usize::MAX, resumes: 0 };
                    self.sources = sources;
                    vec![Effect::Draw]
                }
                (State::Idle, Event::Frame { wanted: true, sources, .. }) => {
                    self.state = State::Filling { frames: 1, best: usize::MAX, resumes: 0 };
                    // differs from dev: the run notes the sources it started for.
                    self.sources = sources;
                    vec![Effect::Draw]
                }
                (State::Filling { frames, best, resumes }, Event::Frame { now, wanted: true, missing, .. }) => {
                    let frames = frames.saturating_add(1).min(12 + 1);
                    if frames <= 12 {
                        self.state = State::Filling { frames, best, resumes };
                        return vec![Effect::Draw];
                    }
                    let (best, resumes) = if missing < best { (missing, 0) } else { (best, resumes) };
                    if missing == 0 {
                        self.state = State::Idle;
                        return vec![];
                    }
                    if resumes >= 3 {
                        self.state = State::Stalled;
                        return vec![];
                    }
                    self.seq = self.seq.wrapping_add(1);
                    let attempt = self.seq;
                    let resumes = resumes + 1;
                    let due = now + 3.0 / 60.0 * f64::from(1u32 << (resumes - 1));
                    self.state = State::Waiting { attempt, due, best, resumes };
                    self.wake = Some((attempt, due));
                    vec![Effect::WakeAt(due)]
                }
                (State::Waiting { .. } | State::Stalled, Event::Frame { wanted: true, .. }) => vec![],
                (State::Waiting { attempt, due, best, resumes }, Event::Due { attempt: a, now, sources })
                    if attempt == a && now >= due =>
                {
                    // differs from dev: the deadline comes due in a frame for other sources,
                    // which starts their own run instead of resuming this one.
                    if sources != self.sources {
                        self.state = State::Filling { frames: 1, best: usize::MAX, resumes: 0 };
                        self.sources = sources;
                        return vec![Effect::Draw];
                    }
                    self.state = State::Filling { frames: 1, best, resumes };
                    vec![Effect::Draw]
                }
                _ => vec![],
            }
        }

        /// `Runtime::virtual_filling` after `virtual_demand`: whether it drew, the deadlines it
        /// armed, and whether it counted the unfilled rows.
        pub(super) fn virtual_filling(&mut self, now: f64, wanted: bool, moved: bool, sources: u64, unfilled: usize) -> (bool, Vec<f64>, bool) {
            if !wanted {
                self.step(Event::Frame { now, wanted: false, missing: 0, moved: false, sources });
                return (false, vec![], false);
            }
            if let Some(attempt) = self.wake(now) {
                return (!self.step(Event::Due { attempt, now, sources }).is_empty(), vec![], false);
            }
            let counted = self.at_bound();
            let missing = if counted { unfilled } else { 0 };
            let (mut draw, mut wakes) = (false, vec![]);
            for effect in self.step(Event::Frame { now, wanted, missing, moved, sources }) {
                match effect {
                    Effect::Draw => draw = true,
                    Effect::WakeAt(due) => wakes.push(due),
                }
            }
            (draw, wakes, counted)
        }
    }
}

/// One frame of a walk.
#[derive(Clone, Copy)]
struct Step {
    wanted: bool,
    moved: bool,
    sources: u64,
    /// Jump the clock to the armed deadline first (a second on, when none is armed).
    to_deadline: bool,
}

/// What a walk's letter stands for: the frames it draws, and how it changes the unfilled count.
fn letter(c: char) -> (Vec<Step>, bool) {
    let run = |sources, to_deadline| {
        (0..13).map(|i| Step { wanted: true, moved: false, sources, to_deadline: to_deadline && i == 0 }).collect()
    };
    match c {
        // A run's worth of frames for source A, or for source B.
        'a' => (run(1, false), false),
        'b' => (run(2, false), false),
        // The armed deadline comes due in a frame for A, or for B, then a run's worth more.
        'w' => (run(1, true), false),
        'x' => (run(2, true), false),
        // A frame with nothing to fill.
        'i' => (vec![Step { wanted: false, moved: false, sources: 1, to_deadline: false }], false),
        // A frame whose list's viewport moved.
        'm' => (vec![Step { wanted: true, moved: true, sources: 1, to_deadline: false }], false),
        // The unfilled range shrinks by a row, then a run's worth of frames for A.
        'p' => (run(1, false), true),
        _ => unreachable!(),
    }
}

/// Every walk up to six letters long over frames for two sources, deadlines that come due in a
/// frame for either, idle frames, moved viewports and progress, each frame driven through
/// [`Fill::frame`] and compared with the reference after every frame: what it drew, the
/// deadline it armed and holds, whether it counted the unfilled rows, and where the run stands.
#[test]
fn every_walk_over_two_sources_matches_the_reference() {
    const LETTERS: [char; 7] = ['a', 'b', 'w', 'x', 'i', 'm', 'p'];
    const LONGEST: u32 = 6;
    let mut walks = 0;
    for len in 1..=LONGEST {
        for n in 0..LETTERS.len().pow(len) {
            let walk: String = (0..len).map(|i| LETTERS[n / LETTERS.len().pow(i) % LETTERS.len()]).collect();
            check_walk(&walk);
            walks += 1;
        }
    }
    assert!(walks > 100_000, "{walks} walks");
}

fn check_walk(walk: &str) {
    use std::cell::Cell;
    let (mut fill, mut dev) = (Fill::default(), reference::Dev::new());
    let (mut now, mut missing) = (0.0, 8usize);
    let mut frame_no = 0;
    for c in walk.chars() {
        let (steps, shrinks) = letter(c);
        if shrinks { missing = missing.saturating_sub(1).max(1); }
        for step in steps {
            frame_no += 1;
            now = match (step.to_deadline, dev.wake) {
                (true, Some((_, due))) => due,
                (true, None) => now + 1.0,
                (false, _) => now + 1.0 / 120.0,
            };
            let counted = Cell::new(false);
            let demand = Demand { wanted: step.wanted, moved: step.moved, sources: Sources(step.sources) };
            let effects = fill.frame(now, demand, || {
                counted.set(true);
                missing
            });
            let drew = effects.clone().into_iter().any(|e| e == FillEffect::Draw);
            let wakes: Vec<f64> = effects.into_iter().filter_map(|e| match e { FillEffect::WakeAt(due) => Some(due), _ => None }).collect();
            let want = dev.virtual_filling(now, step.wanted, step.moved, step.sources, missing);
            let at = format!("frame {frame_no} of walk {walk:?}: fill {:?}, reference {:?}", fill.state(), dev.state);
            assert_eq!((drew, &wakes, counted.get()), (want.0, &want.1, want.2), "draw, deadlines, counted at {at}");
            assert_eq!(fill.wake.map(|(_, due)| due), dev.wake.map(|(_, due)| due), "armed deadline at {at}");
            let same = match (fill.state(), dev.state) {
                (State::Idle, reference::State::Idle) => true,
                (State::Filling { frames, best, resumes, sources }, reference::State::Filling { frames: f, best: b, resumes: r }) =>
                    (frames, best, resumes, sources) == (f, b, r, Sources(dev.sources)),
                (State::Waiting { due, best, resumes, sources, .. }, reference::State::Waiting { due: d, best: b, resumes: r, .. }) =>
                    (due, best, resumes, sources) == (d, b, r, Sources(dev.sources)),
                (State::Stalled { sources }, reference::State::Stalled) => sources == Sources(dev.sources),
                _ => false,
            };
            assert!(same, "state at {at}");
        }
    }
}

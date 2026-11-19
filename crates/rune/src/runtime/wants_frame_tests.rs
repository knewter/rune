//! `Runtime::wants_frame`: what a host that drives its own frames (`LayerHost` on Android and
//! iOS) asks before it stops ticking.
use crate::{Cx, El, Env, Runtime, col, text};

fn view(s: &u32, _: &mut Cx<u32>) -> El {
    col().child(text(format!("count {s}")))
}

fn settled() -> Runtime<u32> {
    let mut rt = Runtime::new(0, view, (200.0, 100.0), Env::default());
    for _ in 0..60 {
        if !rt.frame(1.0 / 60.0) && !rt.wants_frame() {
            return rt;
        }
    }
    panic!("the window never settled");
}

/// A host changing the state after a frame (an embedding view placing its native pages, a
/// platform answer) queues no input, and the window still owes a frame for it.
#[test]
fn a_state_change_after_the_frame_asks_for_the_next() {
    let mut rt = settled();
    rt.update(|count, _| *count += 1);
    assert!(!rt.has_pending_input(), "no input: queued input alone would let the host rest");
    assert!(rt.wants_frame(), "the changed state is not drawn yet");
    rt.frame(1.0 / 60.0);
    assert!(!rt.wants_frame(), "drawn");
}

/// An invalidation (a system setting, the build marking a video's state changed) re-runs the
/// view next frame, which must come.
#[test]
fn an_invalidation_asks_for_a_frame() {
    let mut rt = settled();
    rt.invalidate();
    assert!(rt.wants_frame());
}

/// A new size owes a layout, which no input asks for.
#[test]
fn a_new_viewport_asks_for_a_frame() {
    let mut rt = settled();
    rt.set_viewport((300.0, 120.0), 1.0);
    assert!(rt.wants_frame());
}

//! `El::holds_shortcuts`: the system's shortcuts are held while the element has focus, and
//! with any `Tx::inhibit_shortcuts(true)`, and given back when neither holds them.
use super::*;
use crate::{Cx, El, Env, Key, KeyEvent, Mods, col, el};

#[derive(Default)]
struct S {
    ask: Option<bool>,
}

fn view(_: &S, _: &mut Cx<S>) -> El {
    col()
        .size(300.0, 200.0)
        .child(el().key("screen").size(300.0, 100.0).focusable(true).holds_shortcuts(true))
        .child(el().key("button").size(300.0, 100.0).focusable(true).on_press(|_: &mut S| {}))
}

fn click(rt: &mut Runtime<S>, y: f32) {
    rt.pointer_down(150.0, y);
    rt.pointer_up(150.0, y);
    rt.frame(0.0);
}

#[test]
fn focus_on_a_holding_element_holds_the_shortcuts_and_leaving_gives_them_back() {
    let mut rt = Runtime::new(S::default(), view, (300.0, 200.0), Env::new(true, 1.0));
    rt.frame(0.0);
    assert_eq!(rt.take_inhibit_shortcuts(), None, "nothing is held before anything is focused");
    click(&mut rt, 50.0);
    assert_eq!(rt.take_inhibit_shortcuts(), Some(true), "clicking into the screen holds them");
    rt.frame(0.0);
    assert_eq!(rt.take_inhibit_shortcuts(), None, "the shell is told once, not every frame");
    click(&mut rt, 150.0);
    assert_eq!(rt.take_inhibit_shortcuts(), Some(false), "focus on the button gives them back");
    rt.key(KeyEvent { key: Key::Tab, mods: Mods { shift: true, ..Mods::default() } });
    rt.frame(0.0);
    assert_eq!(rt.take_inhibit_shortcuts(), Some(true), "keyboard focus holds them as a click does");
}

#[test]
fn a_tx_hold_outlasts_focus_leaving() {
    let mut rt = Runtime::new(S::default(), view, (300.0, 200.0), Env::new(true, 1.0));
    rt.frame(0.0);
    rt.update(|s, tx| {
        s.ask = Some(true);
        tx.inhibit_shortcuts(true);
    });
    rt.frame(0.0);
    assert_eq!(rt.take_inhibit_shortcuts(), Some(true));
    click(&mut rt, 50.0);
    click(&mut rt, 150.0);
    assert_eq!(rt.take_inhibit_shortcuts(), None, "full screen's hold stays while focus moves");
    rt.update(|_, tx| tx.inhibit_shortcuts(false));
    rt.frame(0.0);
    assert_eq!(rt.take_inhibit_shortcuts(), Some(false));
}

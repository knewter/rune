use super::{nearest, Toward};
use crate::edit::Key;
use crate::{col, el, row, Cx, El, Env, KeyEvent, Mods, Runtime};

fn button(key: &str) -> El { el().key(key).size(100.0, 40.0).focusable(true).role("button") }

#[test]
fn right_prefers_the_row_it_shares_over_a_nearer_one_below() {
    let from = [0.0, 0.0, 100.0, 40.0];
    let same_row = (1, [300.0, 0.0, 100.0, 40.0]);
    let below_right = (2, [110.0, 60.0, 100.0, 40.0]);
    assert_eq!(nearest(from, Toward::Right, [below_right, same_row]), Some(1));
}

#[test]
fn nothing_beyond_the_edge_is_none_and_a_child_is_not_beyond_its_parent() {
    let card = [0.0, 0.0, 200.0, 100.0];
    let inside = (1, [150.0, 10.0, 40.0, 40.0]);
    assert_eq!(nearest(card, Toward::Right, [inside]), None);
    assert_eq!(nearest(card, Toward::Up, [(2, [0.0, 200.0, 50.0, 50.0])]), None);
}

#[test]
fn down_takes_the_nearest_below_by_its_centre() {
    let from = [100.0, 0.0, 100.0, 40.0];
    let under = (1, [100.0, 50.0, 100.0, 40.0]);
    let far_left = (2, [0.0, 50.0, 40.0, 40.0]);
    assert_eq!(nearest(from, Toward::Down, [far_left, under]), Some(1));
}

/// A sidebar beside a grid of cards, as Dozer's Home is drawn on a television.
#[derive(Default)]
struct Home;

fn home(_: &Home, _: &mut Cx<Home>) -> El {
    let side = col().key("side").w(100.0).focus_group()
        .child(button("nav-home"))
        .child(button("nav-bots"))
        .child(button("nav-feed"));
    let cards = col().key("cards").w(200.0)
        .child(row().child(button("card-a")).child(button("card-b")))
        .child(row().child(button("card-c")).child(button("card-d")));
    row().key("root").w_full().h_full().child(side).child(cards)
}

fn press(rt: &mut Runtime<Home>, key: Key) {
    rt.key(KeyEvent { key, mods: Mods::default() });
    rt.frame(0.0);
}

fn focused(rt: &Runtime<Home>) -> String {
    rt.elements().into_iter().find(|e| e.focused).and_then(|e| e.key).unwrap_or_default()
}

fn television() -> Runtime<Home> {
    let mut rt = Runtime::new(Home, home, (300.0, 200.0), Env::new(true, 1.0));
    rt.set_spatial_arrows(true);
    rt.frame(0.0);
    rt
}

#[test]
fn a_remote_crosses_from_the_sidebar_into_the_content_and_back() {
    let mut rt = television();
    press(&mut rt, Key::Down);
    assert_eq!(focused(&rt), "nav-home", "the first arrow focuses the first control");
    press(&mut rt, Key::Right);
    assert_eq!(focused(&rt), "card-a");
    press(&mut rt, Key::Right);
    assert_eq!(focused(&rt), "card-b");
    press(&mut rt, Key::Down);
    assert_eq!(focused(&rt), "card-d");
    press(&mut rt, Key::Left);
    assert_eq!(focused(&rt), "card-c");
    press(&mut rt, Key::Left);
    assert_eq!(focused(&rt), "nav-bots", "back to the sidebar row it sits beside");
    press(&mut rt, Key::Down);
    assert_eq!(focused(&rt), "nav-feed");
    press(&mut rt, Key::Down);
    assert_eq!(focused(&rt), "nav-feed", "an edge stays put rather than wrapping");
    assert!(rt.elements().iter().any(|e| e.focused && e.focus_visible), "focus shows");
}

#[test]
fn without_the_setting_arrows_keep_to_their_group() {
    let mut rt = Runtime::new(Home, home, (300.0, 200.0), Env::new(true, 1.0));
    rt.frame(0.0);
    press(&mut rt, Key::Tab);
    assert_eq!(focused(&rt), "nav-home");
    press(&mut rt, Key::Right);
    assert_eq!(focused(&rt), "nav-bots", "a keyboard's Right walks the group, as before");
}

/// A search field over a card: Down leaves a single line, Left and Right stay in it.
struct Search { query: crate::TextEdit }

fn search(s: &Search, _: &mut Cx<Search>) -> El {
    col().key("root").w_full().h_full()
        .child(crate::text_input(&s.query, crate::TextStyle::new(14.0, 400.0)).key("query").size(200.0, 40.0)
            .single_line().on_edit(|s: &mut Search| &mut s.query))
        .child(button("card"))
}

#[test]
fn down_leaves_a_single_line_field_and_left_stays_in_it() {
    let mut query = crate::TextEdit::new("abc");
    query.focused = true;
    let mut rt = Runtime::new(Search { query }, search, (300.0, 200.0), Env::new(true, 1.0));
    rt.set_spatial_arrows(true);
    rt.frame(0.0);
    let step = |rt: &mut Runtime<Search>, key| { rt.key(KeyEvent { key, mods: Mods::default() }); rt.frame(0.0); };
    let focused = |rt: &Runtime<Search>| rt.elements().into_iter().find(|e| e.focused).and_then(|e| e.key).unwrap_or_default();
    step(&mut rt, Key::Left);
    assert_eq!(focused(&rt), "query", "Left moves the caret");
    step(&mut rt, Key::Down);
    assert_eq!(focused(&rt), "card");
    step(&mut rt, Key::Up);
    assert_eq!(focused(&rt), "query");
}

/// A many-line field over a card, as Dozer's composer sits over Home's suggestions.
struct Composer { draft: crate::TextEdit }

fn composer(s: &Composer, _: &mut Cx<Composer>) -> El {
    col().key("root").w_full().h_full()
        .child(crate::text_input(&s.draft, crate::TextStyle::new(14.0, 400.0)).key("draft").size(200.0, 80.0)
            .on_edit(|s: &mut Composer| &mut s.draft))
        .child(button("suggestion"))
}

#[test]
fn a_field_not_being_typed_in_passes_the_remote_on() {
    let mut draft = crate::TextEdit::new("one\ntwo");
    draft.focused = true;
    let mut rt = Runtime::new(Composer { draft }, composer, (300.0, 200.0), Env::new(true, 1.0));
    rt.set_spatial_arrows(true);
    rt.frame(0.0);
    let step = |rt: &mut Runtime<Composer>, key| { rt.key(KeyEvent { key, mods: Mods::default() }); rt.frame(0.0); };
    let focused = |rt: &Runtime<Composer>| rt.elements().into_iter().find(|e| e.focused).and_then(|e| e.key).unwrap_or_default();
    step(&mut rt, Key::Down);
    assert_eq!(focused(&rt), "draft", "typing in it, Down moves between its lines");
    rt.set_fields_keep_arrows(false);
    step(&mut rt, Key::Down);
    assert_eq!(focused(&rt), "suggestion");
}

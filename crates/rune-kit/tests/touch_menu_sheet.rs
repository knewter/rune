//! Under a finger: menu rows a finger tall, a description that wraps, and in a narrow window
//! the menu as a bottom sheet inside the safe area. With a pointer nothing moves.
use rune::{Cx, El, Env, Runtime, text};
use rune_kit::{Kit, Menu, MenuItem, RadioGroup, RadioItem, Sheet, root, sheet};

#[derive(Default)]
struct App {
    touch: f32,
    open: bool,
    picked: u32,
    panel: bool,
}

const REASON: &str = "This machine has no voice to read replies with, so there is nothing to turn on here yet";

fn view(app: &App, _: &mut Cx<App>) -> El {
    let mut k = Kit::neutral(false);
    k.touch_target = app.touch;
    let mut layer = Vec::new();
    let body = Menu::new("more")
        .trigger(text("More"))
        .item(MenuItem::new("Find in conversation").shortcut("Ctrl+F").on_select(|a: &mut App| a.picked += 1))
        .item(MenuItem::new("Rename…"))
        .item(MenuItem::new("Read replies aloud").description(REASON).disabled(true))
        .open(app.open)
        .on_close(|a: &mut App| a.open = false)
        .build(&k, &mut layer);
    Sheet::new("panel")
        .title("Cost")
        .child(text("$0.42 so far").key("panel-line"))
        .open(app.panel)
        .on_close(|a: &mut App| a.panel = false)
        .build(&k, &mut layer);
    let radios = RadioGroup::new()
        .label("Model")
        .item(RadioItem::new("Dozer picks", true))
        .item(RadioItem::new("Scripted", false))
        .build(&k)
        .key("radios");
    root(rune::col().w_full().h_full().child(body).child(radios), layer)
}

fn runtime(touch: f32, sheets: Option<[f32; 4]>) -> Runtime<App> {
    sheet::present_menus(sheets);
    let mut rt = Runtime::new(App { touch, open: true, ..App::default() }, view, (412.0, 915.0), Env::new(false, 1.0));
    rt.frame(0.0);
    rt.frame(1.0);
    rt
}

fn element(rt: &Runtime<App>, key: &str) -> rune::runtime::ElementInfo {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap_or_else(|| panic!("{key}"))
}

fn find(rt: &Runtime<App>, key: &str) -> Option<rune::runtime::ElementInfo> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting)
}

fn within(rt: &Runtime<App>, parent: &str, key: &str) -> rune::runtime::ElementInfo {
    let prefix = format!("{}/", element(rt, parent).path);
    rt.elements().into_iter().find(|e| e.path.starts_with(&prefix) && e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("{parent}/{key}"))
}

#[test]
fn with_a_pointer_a_menu_is_the_popover_it_was() {
    let rt = runtime(0.0, None);
    assert_eq!(element(&rt, "more-item0").rect[3], 28.0);
    assert!(find(&rt, "more-menu").is_some() && find(&rt, "more-sheet").is_none());
    let reason = within(&rt, "more-item2", "description");
    assert!(reason.rect[3] < 20.0, "one cut line, as before: {:?}", reason.rect);
    let radios = element(&rt, "radios");
    let first = within(&rt, "radios", "item0").rect;
    let second = within(&rt, "radios", "item1").rect;
    assert_eq!((first[3], second[1] - first[1]), (20.0, 32.0), "{radios:?}");
}

#[test]
fn under_a_finger_a_popover_menus_rows_are_a_fingers_height_and_a_reason_wraps() {
    let rt = runtime(48.0, None);
    assert!(find(&rt, "more-menu").is_some() && find(&rt, "more-sheet").is_none(), "a wide touched window keeps popovers");
    for item in ["more-item0", "more-item1", "more-item2"] {
        assert!(element(&rt, item).rect[3] >= 48.0, "{item}: {:?}", element(&rt, item).rect);
    }
    let row = element(&rt, "more-item2").rect;
    let reason = within(&rt, "more-item2", "description");
    assert!(reason.rect[3] > 20.0, "the reason takes the lines it needs: {:?}", reason.rect);
    assert!(reason.rect[0] + reason.rect[2] <= row[0] + row[2] + 0.5, "inside its row: {:?} in {row:?}", reason.rect);
    for item in ["item0", "item1"] {
        assert!(within(&rt, "radios", item).rect[3] >= 48.0, "a radio row is a finger tall");
    }
}

#[test]
fn in_a_narrow_touched_window_a_menu_is_a_bottom_sheet_inside_the_safe_area() {
    // A phone: 24 points of status bar, 20 of gesture bar.
    let mut rt = runtime(48.0, Some([24.0, 0.0, 20.0, 0.0]));
    assert!(find(&rt, "more-menu").is_none(), "no popover");
    let sheet = element(&rt, "more-sheet").rect;
    assert!((sheet[2] - 412.0).abs() < 0.5 && sheet[0].abs() < 0.5, "as wide as the window: {sheet:?}");
    assert!(sheet[1] >= 24.0 + 8.0, "under the status bar: {sheet:?}");
    assert!(sheet[1] + sheet[3] >= 915.0, "runs under the gesture bar: {sheet:?}");
    let mut last = 0.0_f32;
    for item in ["more-item0", "more-item1", "more-item2"] {
        let r = element(&rt, item).rect;
        assert!(r[3] >= 48.0, "{item} is {} tall", r[3]);
        last = last.max(r[1] + r[3]);
    }
    assert!((48.0..=56.0).contains(&element(&rt, "more-item1").rect[3]), "a plain row is 48 to 56 points");
    assert!(last <= 915.0 - 20.0 + 0.5, "the last row is clear of the gesture bar: {last}");
    let reason = within(&rt, "more-item2", "description");
    assert!(reason.rect[3] > 20.0 && reason.rect[0] + reason.rect[2] <= 412.0, "the reason wraps: {:?}", reason.rect);
    // A row with a shortcut beside it is as tall as one without.
    assert_eq!(element(&rt, "more-item0").rect[3], element(&rt, "more-item1").rect[3]);
    // Picking a row does what it did in the popover.
    let r = element(&rt, "more-item0").rect;
    rt.pointer_down(r[0] + 40.0, r[1] + 20.0);
    rt.pointer_up(r[0] + 40.0, r[1] + 20.0);
    rt.frame(2.0);
    assert_eq!(rt.state.picked, 1);
    // A press on the scrim, above the sheet, closes it.
    rt.pointer_down(200.0, 100.0);
    rt.pointer_up(200.0, 100.0);
    rt.frame(3.0);
    assert!(!rt.state.open, "the scrim closes the sheet");
    sheet::present_menus(None);
}

#[test]
fn escape_and_a_drag_down_on_the_grabber_close_a_sheet() {
    let mut rt = runtime(48.0, Some([24.0, 0.0, 20.0, 0.0]));
    rt.key(rune::KeyEvent { key: rune::Key::Escape, mods: Default::default() });
    rt.frame(2.0);
    assert!(!rt.state.open, "Escape, which a phone's Back is, closes the sheet");
    rt.update(|a, _| a.panel = true);
    rt.frame(3.0);
    rt.frame(4.0);
    let sheet = element(&rt, "panel-sheet").rect;
    assert!(sheet[1] + sheet[3] >= 915.0 && sheet[1] > 600.0, "a short sheet sits at the bottom: {sheet:?}");
    let line = element(&rt, "panel-line").rect;
    assert!(line[1] + line[3] <= 915.0 - 20.0 + 0.5, "its content is clear of the gesture bar: {line:?}");
    let grab = (206.0, sheet[1] + 12.0);
    // A short drag is not a dismissal.
    rt.pointer_down(grab.0, grab.1);
    rt.pointer_move(grab.0, grab.1 + 20.0);
    rt.pointer_up(grab.0, grab.1 + 20.0);
    rt.frame(5.0);
    assert!(rt.state.panel);
    rt.pointer_down(grab.0, grab.1);
    rt.pointer_move(grab.0, grab.1 + 40.0);
    rt.pointer_move(grab.0, grab.1 + 90.0);
    rt.pointer_up(grab.0, grab.1 + 90.0);
    rt.frame(6.0);
    assert!(!rt.state.panel, "dragged down, the sheet closes");
    sheet::present_menus(None);
}

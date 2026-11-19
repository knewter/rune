use rune::{Cx, El, Env, Runtime, col};
use rune_kit::*;

fn controls(touch_target: &f32, cx: &mut Cx<f32>) -> El {
    let mut k = Kit::neutral(cx.system_dark());
    k.touch_target = *touch_target;
    col()
        .items_start()
        .child(Button::new("Save").control_size(ControlSize::Small).build(&k).key("button"))
        .child(IconButton::new(k.glyph.copy).build(&k).key("icon"))
        .child(Switch::new(false).label("Camera off").hide_label().build(&k).key("switch"))
        .child(Checkbox::new(false).label("Remember").build(&k).key("checkbox"))
        .child(Collapsible::new("More", false).build(&k).key("disclosure"))
}

fn sizes(touch_target: f32) -> Vec<(String, f32, f32)> {
    let mut rt = Runtime::new(touch_target, controls, (400.0, 600.0), Env { system_dark: false, scale: 2.0 });
    rt.frame(0.0);
    rt.elements()
        .into_iter()
        .filter(|e| matches!(e.role, "button" | "switch" | "checkbox"))
        .map(|e| (e.label.unwrap_or_default(), e.rect[2], e.rect[3]))
        .collect()
}

#[test]
fn a_touch_target_makes_every_control_a_fingers_size_and_none_leaves_them_alone() {
    let touched = sizes(48.0);
    assert_eq!(touched.len(), 5, "{touched:?}");
    for (label, w, h) in &touched {
        assert!(*w >= 48.0 && *h >= 48.0, "{label} is {w}x{h} under a finger");
    }
    let pointed = sizes(0.0);
    assert!(pointed.iter().any(|(_, _, h)| *h < 48.0), "without a target the kit keeps its sizes: {pointed:?}");
}

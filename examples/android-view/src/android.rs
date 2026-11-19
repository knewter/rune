//! The JNI side of `RuneView.kt`. Every call arrives on the UI thread, which owns the one
//! [`LayerHost`] for the life of the process: an activity (and its view and surface) comes and
//! goes around it. Text is exchanged in UTF-16 offsets, as Android counts them.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use accesskit_android::InjectingAdapter;
use accesskit_android::jni::objects::{GlobalRef, JFloatArray, JIntArray, JObject, JString};
use accesskit_android::jni::sys::{jboolean, jfloat, jint, jintArray, jlong, jstring};
use accesskit_android::jni::{JNIEnv, JavaVM};
use rune::embed::LayerHost;
use rune::clipboard::MemoryClipboard;
use rune::{AccessibilityLink, FocusedField, Ime, Key, KeyEvent, Mods, TouchPhase};

use crate::{State, spec};

#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_fromSurface(env: *mut accesskit_android::jni::sys::JNIEnv, surface: accesskit_android::jni::sys::jobject) -> *mut c_void;
    fn ANativeWindow_release(window: *mut c_void);
}

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(prio: i32, tag: *const std::ffi::c_char, text: *const std::ffi::c_char) -> i32;
}

fn log(msg: &str) {
    let line = std::ffi::CString::new(msg).unwrap_or_default();
    // SAFETY: two valid C strings.
    unsafe { __android_log_write(4, c"RuneView".as_ptr(), line.as_ptr()) };
}

/// The runtime and what it is attached to now.
struct Host {
    layer: LayerHost<State>,
    /// The `ANativeWindow` the surface draws into; null while Android has none for us.
    window: *mut c_void,
    density: f32,
    link: AccessibilityLink,
    adapter: Option<InjectingAdapter>,
    /// The focused field after the last frame, for `RuneView.syncInput`.
    field: Option<FocusedField>,
    wake_after_ms: i64,
    frames: u64,
    /// Which `RuneView` the window belongs to: a new activity's surface can arrive before the old
    /// one's goes, and the old one's going must not take the new window with it.
    owner: jint,
    /// Rune's clipboard: Android's goes in before a paste, and a copy comes out after a frame.
    clipboard: MemoryClipboard,
    /// The clipboard text Android last heard from us, or put there for a paste.
    clipboard_seen: Option<String>,
}

/// A press asked for a tick (`crate::haptic`); the view performs it after the frame.
static HAPTIC: AtomicBool = AtomicBool::new(false);
/// Whether the app has something Back would undo, so the view takes Back (and its predictive
/// gesture) instead of leaving the activity.
pub(crate) static CAN_GO_BACK: AtomicBool = AtomicBool::new(false);

pub(crate) fn request_haptic() { HAPTIC.store(true, Ordering::Relaxed); }

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}
static VM: OnceLock<JavaVM> = OnceLock::new();
/// The current `RuneView`, which a wake from any thread posts a frame to.
static VIEW: Mutex<Option<GlobalRef>> = Mutex::new(None);
static START: OnceLock<std::time::Instant> = OnceLock::new();

fn waker() -> rune::Waker {
    Arc::new(|| {
        let Some(vm) = VM.get() else { return };
        let Some(view) = VIEW.lock().ok().and_then(|v| v.clone()) else { return };
        if let Ok(mut env) = vm.attach_current_thread() {
            let _ = env.call_method(view.as_obj(), "wakeFromNative", "()V", &[]);
            if env.exception_check().unwrap_or(false) { let _ = env.exception_clear(); }
        }
    })
}

fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|h| h.borrow_mut().as_mut().map(f))
}

/// A new view gets its own AccessKit adapter and link: the runtime's tree goes whole to the
/// first client of the new one.
fn attach_accessibility(env: &mut JNIEnv, host: &mut Host) {
    let Some(view) = VIEW.lock().ok().and_then(|v| v.clone()) else { return };
    host.adapter = None;
    host.link = AccessibilityLink::with_wake(waker());
    let (activation, actions) = host.link.handlers();
    host.adapter = Some(InjectingAdapter::new(env, view.as_obj(), activation, Logged(actions)));
}

/// Rune's action handler, logging what an assistive client asked for (`adb logcat -s RuneView`).
struct Logged(rune::AccessibilityActions);
impl rune::accesskit::ActionHandler for Logged {
    fn do_action(&mut self, request: rune::accesskit::ActionRequest) {
        log(&format!("a11y action {:?} on {:?}", request.action, request.target_node));
        self.0.do_action(request);
    }
}

fn utf16_len(s: &str) -> usize { s.encode_utf16().count() }

/// The byte offset of UTF-16 offset `u` in `s`, clamped to its end.
fn byte_of_utf16(s: &str, u: usize) -> usize {
    let mut n = 0;
    for (b, c) in s.char_indices() {
        if n >= u { return b; }
        n += c.len_utf16();
    }
    s.len()
}

/// The field as Android's `Editable` holds it: the composition sits in the text at the caret.
/// Returns the text, the selection and the composing span, in UTF-16 offsets.
fn android_view_of(f: &FocusedField) -> (String, (usize, usize), Option<(usize, usize)>) {
    let Some((pre, cur)) = f.preedit.as_ref().filter(|(p, _)| !p.is_empty()) else {
        return (f.text.clone(), (utf16_len(&f.text[..f.anchor]), utf16_len(&f.text[..f.focus])), None);
    };
    let caret = f.focus;
    let text = format!("{}{}{}", &f.text[..caret], pre, &f.text[caret..]);
    let c0 = utf16_len(&f.text[..caret]);
    let c1 = c0 + utf16_len(pre);
    let sel = cur.map_or((c1, c1), |(a, b)| (c0 + utf16_len(&pre[..a.min(pre.len())]), c0 + utf16_len(&pre[..b.min(pre.len())])));
    (text, sel, Some((c0, c1)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeAttach(mut env: JNIEnv, this: JObject) {
    let _ = START.set(std::time::Instant::now());
    if let Ok(vm) = env.get_java_vm() { let _ = VM.set(vm); }
    if let Ok(global) = env.new_global_ref(&this) {
        if let Ok(mut v) = VIEW.lock() { *v = Some(global); }
    }
    HOST.with(|h| {
        let mut h = h.borrow_mut();
        log(&format!("attach: a new RuneView; runtime {}", if h.is_some() { "kept from before" } else { "not made yet" }));
        if let Some(host) = h.as_mut() { attach_accessibility(&mut env, host); }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeSurfaceCreated(
    mut env: JNIEnv, _this: JObject, surface: JObject, width: jint, height: jint, density: jfloat, dark: jboolean, font_scale: jfloat, owner: jint,
) {
    set_font_scale(font_scale);
    // SAFETY: `surface` is a live `android.view.Surface` passed by the caller.
    let window = unsafe { ANativeWindow_fromSurface(env.get_raw(), surface.as_raw()) };
    if window.is_null() { log("surfaceCreated: no ANativeWindow"); return; }
    let density = density.max(1.0);
    let viewport = (width as f32 / density, height as f32 / density);
    HOST.with(|h| {
        let mut h = h.borrow_mut();
        if let Some(host) = h.as_mut() {
            // SAFETY: the window is ours until surfaceDestroyed releases it.
            if let Err(e) = unsafe { host.layer.set_android_native_window(window) } { log(&format!("set window: {e}")); }
            if !host.window.is_null() { unsafe { ANativeWindow_release(host.window) }; }
            host.window = window;
            host.owner = owner;
            host.density = density;
            host.layer.resize(viewport, density);
            // A new activity brings the configuration it was made with: night mode, font scale.
            let rt = host.layer.runtime_mut();
            rt.set_system_dark(dark != 0);
            rt.invalidate();
            log(&format!("surfaceCreated: {width}x{height} @ {density}, the runtime from before drawing again"));
            return;
        }
        // SAFETY: as above.
        let made = unsafe { LayerHost::from_android_native_window(window, &spec(), State::default(), viewport, density, dark != 0, waker()) };
        match made {
            Ok(mut layer) => {
                let clipboard = rune::clipboard::memory();
                layer.runtime_mut().use_clipboard(&clipboard);
                let mut host = Host {
                    layer, window, density, link: AccessibilityLink::new(), adapter: None, field: None, wake_after_ms: -1, frames: 0,
                    clipboard, clipboard_seen: None, owner,
                };
                attach_accessibility(&mut env, &mut host);
                log(&format!("surfaceCreated: {width}x{height} @ {density}, a new runtime"));
                *h = Some(host);
            }
            Err(e) => {
                log(&format!("LayerHost: {e}"));
                unsafe { ANativeWindow_release(window) };
            }
        }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeSurfaceChanged(
    _env: JNIEnv, _this: JObject, width: jint, height: jint, density: jfloat,
) {
    with_host(|host| {
        host.density = density.max(1.0);
        host.layer.resize((width as f32 / host.density, height as f32 / host.density), host.density);
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeSurfaceDestroyed(_env: JNIEnv, _this: JObject, owner: jint) {
    with_host(|host| {
        if host.owner != owner {
            log("surfaceDestroyed: an older view's surface; the current one keeps drawing");
            return;
        }
        // SAFETY: null drops the swapchain before the window is released.
        let _ = unsafe { host.layer.set_android_native_window(std::ptr::null_mut()) };
        if !host.window.is_null() { unsafe { ANativeWindow_release(host.window) }; }
        host.window = std::ptr::null_mut();
        // A finger down when the surface went never lifts.
        host.layer.runtime_mut().cancel_touch();
        log("surfaceDestroyed: the runtime stays");
    });
}

/// Android's font scale (Settings > Display > Font size) as Rune's text scale.
fn set_font_scale(scale: f32) {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    rune::tok::set_text_scale(scale);
}

fn now() -> f64 { START.get().map_or(0.0, |s| s.elapsed().as_secs_f64()) }

/// `MotionEvent`: the masked action, the index of the pointer it is about, and every pointer.
#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeTouch(
    env: JNIEnv, _this: JObject, action: jint, index: jint, ids: JIntArray, xs: JFloatArray, ys: JFloatArray, time_ms: jlong,
) {
    let n = env.get_array_length(&ids).unwrap_or(0) as usize;
    let (mut id, mut x, mut y) = (vec![0; n], vec![0.0; n], vec![0.0; n]);
    if env.get_int_array_region(&ids, 0, &mut id).is_err()
        || env.get_float_array_region(&xs, 0, &mut x).is_err()
        || env.get_float_array_region(&ys, 0, &mut y).is_err() { return; }
    let time = time_ms as f64 / 1000.0;
    with_host(|host| {
        let d = host.density;
        let rt = host.layer.runtime_mut();
        let one = |rt: &mut rune::Runtime<State>, i: usize, phase| rt.touch(id[i] as u64, phase, x[i] / d, y[i] / d, time);
        let index = (index.max(0) as usize).min(n.saturating_sub(1));
        match action {
            0 | 5 => one(rt, index, TouchPhase::Started),
            1 | 6 => one(rt, index, TouchPhase::Ended),
            2 => (0..n).for_each(|i| one(rt, i, TouchPhase::Moved)),
            3 => (0..n).for_each(|i| one(rt, i, TouchPhase::Cancelled)),
            _ => {}
        }
    });
}

/// A key the view heard (a hardware key, or one the input method sent): `code` is ours,
/// `text` the character for code 0.
#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeKey(mut env: JNIEnv, _this: JObject, code: jint, text: JString, shift: jboolean) {
    let key = match code {
        1 => Key::Enter,
        2 => Key::Backspace,
        3 => Key::Delete,
        4 => Key::Left,
        5 => Key::Right,
        6 => Key::Up,
        7 => Key::Down,
        8 => Key::Escape,
        9 => Key::Tab,
        // Ctrl (or a dedicated key: Copy, Cut, Select all) with a letter.
        10 => match env.get_string(&text).ok().and_then(|s| String::from(s).chars().next()) {
            Some(c) => Key::Command(c.to_ascii_lowercase()),
            None => return,
        },
        // Paste: `text` is what Android's clipboard holds.
        11 => {
            let Ok(s) = env.get_string(&text).map(String::from) else { return };
            with_host(|host| {
                host.clipboard_seen = Some(s.clone());
                host.layer.runtime_mut().paste(s);
            });
            return;
        }
        _ => match env.get_string(&text) {
            Ok(s) => Key::Text(s.into()),
            Err(_) => return,
        },
    };
    with_host(|host| host.layer.runtime_mut().key(KeyEvent { key, mods: Mods { shift: shift != 0, ..Mods::default() } }));
}

/// The `Editable` behind the `InputConnection` changed: text, selection and composing span in
/// UTF-16 offsets (composing -1 when there is none).
#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeSetField(
    mut env: JNIEnv, _this: JObject, text: JString, sel_start: jint, sel_end: jint, comp_start: jint, comp_end: jint,
) {
    let Ok(text) = env.get_string(&text).map(String::from) else { return };
    let b = |u: jint| byte_of_utf16(&text, u.max(0) as usize);
    let (s0, s1) = (b(sel_start), b(sel_end));
    let ev = if comp_start >= 0 && comp_end > comp_start && b(comp_start) <= s0.min(s1) && s0.max(s1) <= b(comp_end) {
        // The composition is drawn at the caret, out of the text, with its own cursor.
        let (c0, c1) = (b(comp_start), b(comp_end));
        let pre = text[c0..c1].to_owned();
        let rest = format!("{}{}", &text[..c0], &text[c1..]);
        Ime::Replace { text: rest, anchor: c0, focus: c0, preedit: Some((pre, Some((s0 - c0, s1 - c0)))) }
    } else {
        Ime::Replace { text: text.clone(), anchor: s0, focus: s1, preedit: None }
    };
    with_host(|host| host.layer.runtime_mut().ime(ev));
}

/// System bars, cutout and soft keyboard over the view, in pixels.
#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeInsets(_env: JNIEnv, _this: JObject, top: jint, right: jint, bottom: jint, left: jint) {
    with_host(|host| {
        let d = host.density;
        host.layer.set_safe_area([top as f32 / d, right as f32 / d, bottom as f32 / d, left as f32 / d]);
    });
}

/// One `Choreographer` frame. Bit 0: draw again next frame; bit 1: a text field has focus.
#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeFrame(_env: JNIEnv, _this: JObject) -> jint {
    with_host(|host| {
        let out = host.layer.frame();
        host.frames += 1;
        let title = "Rune on a SurfaceView";
        if let Some(update) = host.link.service(host.layer.runtime_mut(), title)
            && let Some(adapter) = host.adapter.as_mut() {
            adapter.update_if_active(|| update);
        }
        let rt = host.layer.runtime_mut();
        // Actions a client asked for run on the next frame.
        let pending = rt.has_pending_input();
        host.field = rt.focused_field();
        host.wake_after_ms = out.wake_after_secs.map_or(-1, |s| (s * 1000.0).ceil() as i64);
        // A copy or cut Rune made since Android last heard.
        let copied = host.clipboard.text().map(|t| t.to_string());
        let clipboard_changed = copied.is_some() && copied != host.clipboard_seen;
        i32::from(out.animating || pending)
            | (i32::from(host.field.is_some()) << 1)
            | (i32::from(clipboard_changed) << 2)
            | (i32::from(HAPTIC.swap(false, Ordering::Relaxed)) << 3)
            | (i32::from(CAN_GO_BACK.load(Ordering::Relaxed)) << 4)
    }).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeWakeAfterMs(_env: JNIEnv, _this: JObject) -> jlong {
    with_host(|host| host.wake_after_ms).unwrap_or(-1)
}

/// The focused field as an `Editable` would hold it, or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeFieldText(env: JNIEnv, _this: JObject) -> jstring {
    let text = with_host(|host| host.field.as_ref().map(|f| android_view_of(f).0)).flatten();
    match text {
        Some(t) => env.new_string(t).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
    }
}

/// What Rune copied, for Android's clipboard; Android now holds it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeTakeClipboard(env: JNIEnv, _this: JObject) -> jstring {
    let text = with_host(|host| {
        let t = host.clipboard.text().map(|t| t.to_string());
        host.clipboard_seen = t.clone();
        t
    }).flatten();
    match text {
        Some(t) => env.new_string(t).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
    }
}

/// Predictive Back: 0 started or moved (`progress` 0..1), 1 committed, 2 cancelled.
#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeBack(_env: JNIEnv, _this: JObject, phase: jint, progress: jfloat) {
    with_host(|host| host.layer.runtime_mut().update(|s: &mut State, _| crate::back(s, phase, progress)));
}

/// `[selStart, selEnd, compStart, compEnd, singleLine, secure, submits, pathHash]`, or empty.
#[unsafe(no_mangle)]
pub extern "system" fn Java_systems_rune_androidtest_RuneView_nativeFieldInfo(env: JNIEnv, _this: JObject) -> jintArray {
    let ints: Vec<jint> = with_host(|host| {
        host.field.as_ref().map(|f| {
            let (_, (s0, s1), comp) = android_view_of(f);
            let (c0, c1) = comp.map_or((-1, -1), |(a, b)| (a as jint, b as jint));
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            f.path.hash(&mut h);
            vec![s0 as jint, s1 as jint, c0, c1, f.single_line as jint, f.secure as jint, f.submits as jint, h.finish() as jint]
        }).unwrap_or_default()
    }).unwrap_or_default();
    let Ok(arr) = env.new_int_array(ints.len() as i32) else { return std::ptr::null_mut() };
    let _ = env.set_int_array_region(&arr, 0, &ints);
    arr.into_raw()
}

/// NativeActivity's entry point, which `android-activity` (through winit) names; this app has
/// no NativeActivity, so it never runs.
#[unsafe(no_mangle)]
fn android_main(_app: rune::android::AndroidApp) {}

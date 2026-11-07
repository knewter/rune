//! Android through NativeActivity: the app's `android_main` hands Rune the activity, and the
//! event loop takes it from here.
//!
//! ```ignore
//! #[cfg(target_os = "android")]
//! #[unsafe(no_mangle)]
//! fn android_main(app: rune::android::AndroidApp) {
//!     rune::android::init(app);
//!     rune::run(AppSpec::new("Hello", (320.0, 200.0), State::default, view));
//! }
//! ```
//!
//! Android takes the native window away when the app goes to the background and gives a new
//! one when it comes back: windows drop their surface on `suspended` and make a new one on
//! the next `resumed` (`window::android`).

use std::sync::OnceLock;

pub use winit::platform::android::activity::AndroidApp;

static APP: OnceLock<AndroidApp> = OnceLock::new();

/// Hands Rune the activity `android_main` was given, before `rune::run`. A second call is
/// ignored: the process has one activity.
pub fn init(app: AndroidApp) {
    let _ = APP.set(app);
}

/// The activity `init` stored.
pub(crate) fn app() -> Result<AndroidApp, String> {
    APP.get().cloned().ok_or_else(|| "rune::android::init was not called from android_main".into())
}

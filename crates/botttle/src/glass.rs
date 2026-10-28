//! The blur behind a see-through window.
//!
//! GPUI's own blur on recent macOS is an `NSVisualEffectView` with its tint
//! layers stripped out, and on macOS 26 and later stripping them takes the
//! blur with it: the window ends up see-through but sharp. This asks the
//! WindowServer to blur behind the window directly instead, which is what
//! iTerm2 does, and it lets the radius be a setting.

use gpui::Window;

use crate::settings::Settings;

#[cfg(target_os = "macos")]
pub fn apply(window: &Window, settings: &Settings) {
    use objc2::{msg_send, runtime::AnyObject};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGSMainConnectionID() -> i32;
        fn CGSSetWindowBackgroundBlurRadius(connection: i32, window: isize, radius: isize) -> i32;
    }

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };

    unsafe {
        let view = handle.ns_view.as_ptr() as *mut AnyObject;
        let ns_window: *mut AnyObject = msg_send![view, window];
        if ns_window.is_null() {
            return;
        }
        let number: isize = msg_send![ns_window, windowNumber];
        let radius = settings.blur_radius() as isize;
        if CGSSetWindowBackgroundBlurRadius(CGSMainConnectionID(), number, radius) != 0 {
            eprintln!("botttle: could not set the window blur");
        }
    }
}

/// Elsewhere GPUI's `Blurred` background does the job on its own.
#[cfg(not(target_os = "macos"))]
pub fn apply(_: &Window, _: &Settings) {}

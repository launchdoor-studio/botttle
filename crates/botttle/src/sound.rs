//! The chime a tab makes when something in it finishes out of sight.

/// One of the system sounds, so it matches the rest of the mac and respects
/// the alert volume set in System Settings.
#[cfg(target_os = "macos")]
const DONE_SOUND: &std::ffi::CStr = c"Glass";

#[cfg(target_os = "macos")]
pub fn play_done() {
    use objc2::{class, msg_send, runtime::AnyObject};

    // SAFETY: plain AppKit class methods, called on the main thread with a
    // NUL-terminated string that outlives the call. `soundNamed:` hands back a
    // shared instance we neither own nor release.
    unsafe {
        let name: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: DONE_SOUND.as_ptr()];
        if name.is_null() {
            return;
        }
        let sound: *mut AnyObject = msg_send![class!(NSSound), soundNamed: name];
        if sound.is_null() {
            return;
        }
        // Already playing for another tab: once is enough.
        let _: bool = msg_send![sound, play];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn play_done() {}

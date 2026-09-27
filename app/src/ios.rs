//! iOS start-up. iOS starts no process, so the runtime runs inside the app and
//! loads its modules from the bundle; the app's state lives in its container.

use std::ffi::{c_char, CStr};
use std::path::PathBuf;

use demo_core::Paths;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

pub fn main() -> Result<(), slint::PlatformError> {
    let bundle = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_default();
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let paths = Paths {
        runtime_bin: PathBuf::new(),
        host_plain_bin: PathBuf::new(),
        host_remote_bin: PathBuf::new(),
        bundled_modules: bundle.join("modules"),
        app_modules: bundle.join("app-modules"),
        data: home.join("Library/Application Support/Logos Core Demo"),
        tmp: std::env::temp_dir().join("logos-core-demo"),
    };
    // An invite to redeem as soon as the runtime runs: a developer's launch
    // (simctl launch with SIMCTL_CHILD_LOGOS_CORE_DEMO_INVITE); iOS gives users no environment.
    let invite = std::env::var("LOGOS_CORE_DEMO_INVITE").ok().filter(|text| !text.is_empty());
    let redeem = invite.is_some();
    crate::run_app(paths, false, invite, None, redeem)
}

/// The text on the general pasteboard: winit gives an iOS app no clipboard.
pub fn pasteboard_text() -> Option<String> {
    objc2::rc::autoreleasepool(|_| unsafe {
        let board: *mut AnyObject = msg_send![class!(UIPasteboard), generalPasteboard];
        let text: *mut AnyObject = msg_send![board, string];
        if text.is_null() {
            return None;
        }
        let utf8: *const c_char = msg_send![text, UTF8String];
        (!utf8.is_null()).then(|| CStr::from_ptr(utf8).to_string_lossy().trim().to_string())
    })
}

// A GUI program on Windows: no console window.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> Result<(), slint::PlatformError> {
    #[cfg(target_os = "ios")]
    return logos_core_demo::ios::main();
    #[cfg(not(target_os = "ios"))]
    logos_core_demo::run(logos_core_demo::desktop_paths(), true)
}

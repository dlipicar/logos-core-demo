// A GUI program on Windows: no console window.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> Result<(), slint::PlatformError> {
    logos_core_demo::run(logos_core_demo::desktop_paths(), true)
}

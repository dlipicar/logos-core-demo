//! Android start-up. The APK ships the runtime's executables and every plugin
//! as lib*.so files in its native library directory, the only place an app
//! may execute from; module directories are laid out in app storage around them.

use std::ffi::{CStr, CString};
use std::io::{BufRead, BufReader, Read};
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};

use demo_core::Paths;
use serde_json::Value;
use slint::android::AndroidApp;

#[no_mangle]
fn android_main(app: AndroidApp) {
    log_to_logcat();
    if let Err(e) = start(app) {
        eprintln!("logos-core-demo: {e}");
    }
}

fn start(app: AndroidApp) -> Result<(), String> {
    let lib_dir = native_library_dir().ok_or("cannot find the native library directory")?;
    let files = app.internal_data_path().ok_or("no internal data path")?;
    let cache = files.parent().map(|p| p.join("cache")).unwrap_or_else(|| files.join("cache"));
    // The runtime and its hosts are lib*.so executables beside this library.
    std::env::set_var("LD_LIBRARY_PATH", &lib_dir);
    let modules = files.join("modules");
    install_modules(&app, &lib_dir, &modules)?;

    let paths = Paths {
        runtime_bin: lib_dir.join("liblogos_runtime.so"),
        host_plain_bin: lib_dir.join("liblogos_host_plain.so"),
        host_remote_bin: lib_dir.join("liblogos_host_remote.so"),
        bundled_modules: modules.join("bundled"),
        app_modules: modules.join("app"),
        data: files.clone(),
        tmp: cache,
    };
    let invite = launch_invite(&app);
    slint::android::init(app).map_err(|e| e.to_string())?;
    crate::run_with(paths, false, invite).map_err(|e| e.to_string())
}

/// The logos-pair: URI the app was opened with (a scanned QR code), if any. Only
/// a cold start sees it: NativeActivity does not forward onNewIntent.
fn launch_invite(app: &AndroidApp) -> Option<String> {
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }.ok()?;
    let read = || -> jni::errors::Result<Option<String>> {
        let mut env = vm.attach_current_thread_permanently()?;
        let activity = unsafe { jni::objects::JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
        let intent = env.call_method(&activity, "getIntent", "()Landroid/content/Intent;", &[])?.l()?;
        if intent.is_null() {
            return Ok(None);
        }
        let data = env.call_method(&intent, "getDataString", "()Ljava/lang/String;", &[])?.l()?;
        if data.is_null() {
            return Ok(None);
        }
        Ok(Some(env.get_string(&jni::objects::JString::from(data))?.into()))
    };
    read().ok().flatten().filter(|uri| uri.starts_with("logos-pair:"))
}

/// This library's own directory: where the package manager extracted the APK's libs.
fn native_library_dir() -> Option<PathBuf> {
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    let addr = native_library_dir as *const libc::c_void;
    if unsafe { libc::dladdr(addr, &mut info) } == 0 || info.dli_fname.is_null() {
        return None;
    }
    let path = unsafe { CStr::from_ptr(info.dli_fname) }.to_string_lossy().into_owned();
    Path::new(&path).parent().map(Path::to_path_buf)
}

/// Rebuilds `<modules>/<group>/<name>/` from assets/modules/index.json on every
/// start: the manifests and sidecars as files, the plugin as a symlink into the
/// native library directory (whose path changes with every install).
fn install_modules(app: &AndroidApp, lib_dir: &Path, modules: &Path) -> Result<(), String> {
    let index: Value = serde_json::from_slice(&read_asset(app, "modules/index.json")?)
        .map_err(|e| format!("modules/index.json: {e}"))?;
    let _ = std::fs::remove_dir_all(modules);
    for (group, entries) in index.as_object().into_iter().flatten() {
        for (name, entry) in entries.as_object().into_iter().flatten() {
            let dir = modules.join(group).join(name);
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for file in entry["files"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                let bytes = read_asset(app, &format!("modules/{group}/{name}/{file}"))?;
                std::fs::write(dir.join(file), bytes).map_err(|e| format!("{file}: {e}"))?;
            }
            if let Some(plugin) = entry["plugin"].as_str() {
                std::os::unix::fs::symlink(lib_dir.join(format!("lib{plugin}")), dir.join(plugin))
                    .map_err(|e| format!("{plugin}: {e}"))?;
            }
        }
    }
    Ok(())
}

fn read_asset(app: &AndroidApp, name: &str) -> Result<Vec<u8>, String> {
    let cname = CString::new(name).map_err(|e| e.to_string())?;
    let mut asset = app.asset_manager().open(&cname).ok_or(format!("no asset {name}"))?;
    let mut bytes = Vec::new();
    asset.read_to_end(&mut bytes).map_err(|e| format!("{name}: {e}"))?;
    Ok(bytes)
}

/// Android discards an app's stdout and stderr; route both, and so the runtime's
/// and every module host's (children inherit them), to logcat.
fn log_to_logcat() {
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return;
    }
    unsafe {
        libc::dup2(fds[1], libc::STDOUT_FILENO);
        libc::dup2(fds[1], libc::STDERR_FILENO);
        libc::close(fds[1]);
    }
    let read = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    std::thread::spawn(move || {
        let tag = c"logos-core-demo";
        for line in BufReader::new(read).lines().map_while(Result::ok) {
            if let Ok(text) = CString::new(line) {
                unsafe { __android_log_write(4 /* INFO */, tag.as_ptr(), text.as_ptr()) };
            }
        }
    });
}

#[link(name = "log")]
extern "C" {
    fn __android_log_write(prio: libc::c_int, tag: *const libc::c_char, text: *const libc::c_char) -> libc::c_int;
}

/// The device's model, for the name peers see.
pub fn device_model() -> Option<String> {
    let mut value = [0 as libc::c_char; 92]; // PROP_VALUE_MAX
    let len = unsafe { libc::__system_property_get(c"ro.product.model".as_ptr(), value.as_mut_ptr()) };
    (len > 0).then(|| unsafe { CStr::from_ptr(value.as_ptr()) }.to_string_lossy().into_owned())
}

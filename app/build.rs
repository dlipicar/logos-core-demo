fn main() {
    slint_build::compile("ui/app.slint").expect("ui/app.slint");
    // liblogos is @rpath-named: a development build finds it where it linked it.
    println!("cargo:rerun-if-env-changed=LOGOS_HOST_LIB_DIR");
    // An APK's libraries find each other in its own directory instead.
    let android = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android");
    if let Some(dir) = std::env::var_os("LOGOS_HOST_LIB_DIR").filter(|_| !android) {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.to_string_lossy());
    }
}

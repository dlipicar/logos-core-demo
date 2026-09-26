fn main() {
    slint_build::compile("ui/app.slint").expect("ui/app.slint");
    // liblogos is @rpath-named: a development build finds it where it linked it.
    println!("cargo:rerun-if-env-changed=LOGOS_HOST_LIB_DIR");
    if let Some(dir) = std::env::var_os("LOGOS_HOST_LIB_DIR") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.to_string_lossy());
    }
}

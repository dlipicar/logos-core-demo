// liblogos is @rpath-named: find it where the build linked it.
fn main() {
    println!("cargo:rerun-if-env-changed=LOGOS_HOST_LIB_DIR");
    if let Some(dir) = std::env::var_os("LOGOS_HOST_LIB_DIR") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.to_string_lossy());
    }
}

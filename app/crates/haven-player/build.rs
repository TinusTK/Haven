//! Tells the linker where libmpv is: Homebrew's folders on macOS, the
//! packaging folder on Windows. Linux finds the distribution's libmpv.so.2
//! by itself.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Windows: mpv.lib is expected in app/packaging/windows/libmpv (see
    // the README there); it is not kept in git.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/windows/libmpv");
        println!("cargo:rerun-if-changed={}", dir.display());
        if dir.join("mpv.lib").exists() {
            println!("cargo:rustc-link-search=native={}", dir.display());
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        for dir in ["/opt/homebrew/lib", "/usr/local/lib"] {
            if std::path::Path::new(dir).join("libmpv.dylib").exists() {
                println!("cargo:rustc-link-search=native={dir}");
            }
        }
    }
}

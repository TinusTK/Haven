//! Tells the linker where libmpv is: the repository's `windows`, `mac` and
//! `ios` folders (on macOS Homebrew's folders if nothing was fetched). Linux finds the distribution's libmpv.so.2
//! by itself.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Windows: mpv.lib is expected in windows/libmpv (see
    // the README there); it is not kept in git.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../windows/libmpv");
        println!("cargo:rerun-if-changed={}", dir.display());
        if dir.join("mpv.lib").exists() {
            println!("cargo:rustc-link-search=native={}", dir.display());
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        // The frameworks fetched by ios/fetch-libmpv.sh: one set for
        // devices, one for the simulator. `libmpv.dylib` there only points
        // the linker at Mpv.framework; the app loads the framework.
        let slice = if std::env::var("TARGET").is_ok_and(|t| t.ends_with("-sim")) { "simulator" } else { "device" };
        let own = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../ios/libmpv").join(slice);
        println!("cargo:rerun-if-changed={}", own.display());
        if std::env::var_os("CARGO_FEATURE_MPV").is_none() {
            return;
        }
        if !own.join("Mpv.framework").exists() {
            panic!("no libmpv for iOS: run ios/fetch-libmpv.sh first");
        }
        println!("cargo:rustc-link-search=native={}", own.display());
        println!("cargo:rustc-link-search=framework={}", own.display());
        for framework in ["OpenGLES", "CoreVideo", "Metal"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // The LGPL libmpv that ships inside Haven.app, if it has been
        // fetched (mac/fetch-libmpv.sh); else Homebrew's,
        // which is fine for trying things on this Mac but not to ship.
        let own = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../mac/libmpv");
        println!("cargo:rerun-if-changed={}", own.display());
        if own.join("libmpv.dylib").exists() {
            println!("cargo:rustc-link-search=native={}", own.display());
            return;
        }
        for dir in ["/opt/homebrew/lib", "/usr/local/lib"] {
            if std::path::Path::new(dir).join("libmpv.dylib").exists() {
                println!("cargo:rustc-link-search=native={dir}");
            }
        }
    }
}

//! Tells the linker where Homebrew keeps libmpv on macOS. Windows and Linux
//! find it through their own means (DLL beside the exe, distro libmpv.so.2).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        for dir in ["/opt/homebrew/lib", "/usr/local/lib"] {
            if std::path::Path::new(dir).join("libmpv.dylib").exists() {
                println!("cargo:rustc-link-search=native={dir}");
            }
        }
    }
}

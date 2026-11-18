fn main() {
    slint_build::compile("ui/app.slint").expect("ui/app.slint failed to compile");

    // macOS: libmpv is found beside the program inside Haven.app, and,
    // for a run straight from the build folder, where it was fetched to.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
        let own = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../mac/libmpv");
        if std::env::var("PROFILE").as_deref() == Ok("debug") {
            if let Ok(own) = own.canonicalize() {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{}", own.display());
            }
        }
    }

    // Put the icon inside haven.exe (File Explorer, taskbar, Alt+Tab).
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=../../assets/icon/haven.ico");
        winresource::WindowsResource::new()
            .set_icon("../../assets/icon/haven.ico")
            .compile()
            .expect("could not embed the Windows icon");
    }
}

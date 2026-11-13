fn main() {
    slint_build::compile("ui/app.slint").expect("ui/app.slint failed to compile");

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

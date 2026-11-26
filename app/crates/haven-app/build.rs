/// Keeps the parts of the UI this build has. Blocks marked
///
/// ```text
/// // addons:begin
/// ...only in builds with add-ons...
/// // addons:else
/// ...only in builds without (the store builds)...
/// // addons:end
/// ```
///
/// are cut to one side or the other (`addons:else` is optional), so a
/// store build's UI holds no add-on page and no word about add-ons.
fn cut_for_build(source: &str, addons: bool) -> String {
    #[derive(PartialEq)]
    enum Part {
        Both,
        WithAddons,
        WithoutAddons,
    }
    let mut part = Part::Both;
    let mut out = String::with_capacity(source.len());
    for (number, line) in source.lines().enumerate() {
        let marker = line.trim();
        let next = match marker {
            "// addons:begin" if part == Part::Both => Some(Part::WithAddons),
            "// addons:else" if part == Part::WithAddons => Some(Part::WithoutAddons),
            "// addons:end" if part != Part::Both => Some(Part::Both),
            "// addons:begin" | "// addons:else" | "// addons:end" => {
                panic!("ui/app.slint line {}: {marker} out of place", number + 1)
            }
            _ => None,
        };
        let keep = match part {
            Part::Both => true,
            Part::WithAddons => addons,
            Part::WithoutAddons => !addons,
        };
        if let Some(next) = next {
            part = next;
            // Keep the line count, so Slint's error lines still match.
            out.push('\n');
            continue;
        }
        if keep {
            out.push_str(line);
        }
        out.push('\n');
    }
    assert!(part == Part::Both, "ui/app.slint: an addons:begin has no addons:end");
    out
}

fn main() {
    let ui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui");
    println!("cargo:rerun-if-changed=ui/app.slint");
    let source = std::fs::read_to_string(ui.join("app.slint")).expect("ui/app.slint is missing");
    let addons = std::env::var_os("CARGO_FEATURE_ADDONS").is_some();
    // The cut copy lives in the build folder, so its paths to fonts and
    // pictures are made absolute.
    let assets = ui.join("../../../assets/");
    let cut = cut_for_build(&source, addons).replace("\"../../../assets/", &format!("\"{}", assets.display().to_string().replace('\\', "/")));
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.slint");
    std::fs::write(&out, cut).expect("could not write the UI for this build");
    slint_build::compile(&out).expect("ui/app.slint failed to compile");

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

    // Android: Haven's library laid out for 16 KB memory pages, which
    // Google Play requires of apps for Android 15 and newer (it runs on
    // 4 KB phones too). Set here because cargo-apk overrides RUSTFLAGS.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-arg=-Wl,-z,max-page-size=16384");
    }

    // Android: libmpv from android/fetch-libmpv.sh, packed into the APK
    // beside Haven (`runtime_libs` in Cargo.toml). Haven links against the
    // stand-in made next to it, which has only mpv's own functions.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android")
        && std::env::var_os("CARGO_FEATURE_PLAYER").is_some()
    {
        let own = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../android/libmpv");
        println!("cargo:rerun-if-changed={}", own.display());
        if !own.join("arm64-v8a/libmpv.so").exists() || !own.join("stub/libmpv.so").exists() {
            panic!("no libmpv for Android: run android/fetch-libmpv.sh first");
        }
        println!("cargo:rustc-link-search=native={}", own.join("stub").display());
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

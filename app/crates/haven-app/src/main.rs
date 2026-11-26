// No console window behind the app on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A debug build can show the phone's layout in a window, to check it.
    let phone = cfg!(debug_assertions) && std::env::var_os("HAVEN_PHONE").is_some();
    haven_app::run(haven_app::desktop_paths()?, phone)
}

// No console window behind the app on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    haven_app::run(haven_app::desktop_paths()?)
}

// No console window behind the app on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{cell::RefCell, error::Error, rc::Rc};

use haven_core::{
    paths::AppPaths,
    // `Theme` is also the name of the UI's colour global.
    settings::{Settings, Theme as ThemeChoice},
    storage::Store,
};
use slint::{ComponentHandle, Model};

#[cfg(debug_assertions)]
mod devtools;
mod library;
mod livetv;
#[cfg(feature = "player")]
mod player;

slint::include_modules!();

fn main() -> Result<(), Box<dyn Error>> {
    #[cfg(debug_assertions)]
    let paths = match std::env::var_os("HAVEN_DATA_DIR") {
        Some(folder) => AppPaths::at(folder)?,
        None => AppPaths::discover()?,
    };
    #[cfg(not(debug_assertions))]
    let paths = AppPaths::discover()?;
    haven_core::diag::start(&paths.data_dir().join("haven.log"));
    let store = Rc::new(Store::open(&paths.database())?);
    let settings = Rc::new(RefCell::new(Settings::load(&store)?));

    // Must run before the first window exists: the player draws video with
    // OpenGL, so the window has to be drawn with OpenGL too.
    // If that cannot be had, carry on: Haven then runs without the player.
    #[cfg(feature = "player")]
    if let Err(err) = slint::BackendSelector::new().require_opengl().select() {
        eprintln!("haven: OpenGL is not available: {err}");
    }

    let ui = AppWindow::new()?;
    ui.set_version(env!("CARGO_PKG_VERSION").into());
    ui.set_data_path(paths.data_dir().display().to_string().into());
    ui.set_log_path(paths.data_dir().join("haven.log").display().to_string().into());
    ui.set_dark(settings.borrow().theme == ThemeChoice::Dark);
    let page_count = ui.get_pages().row_count() as u32;
    ui.set_page(settings.borrow().last_page.min(page_count.saturating_sub(1)) as i32);

    ui.on_dark_toggled({
        let (store, settings) = (store.clone(), settings.clone());
        move |dark| {
            settings.borrow_mut().theme = if dark { ThemeChoice::Dark } else { ThemeChoice::Light };
            save(&settings.borrow(), &store);
        }
    });
    ui.on_page_selected({
        let (store, settings) = (store.clone(), settings.clone());
        move |page| {
            settings.borrow_mut().last_page = page.max(0) as u32;
            save(&settings.borrow(), &store);
        }
    });

    // Network work (add-ons, posters) runs on these threads, never on the
    // UI thread.
    let tasks = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_name("haven-net")
        .enable_all()
        .build()?;
    // Optional keys file beside the database; Settings has fields too.
    let secrets = haven_core::secrets::Secrets::from_file(&paths.data_dir().join("secrets.env"))?;
    let library = library::Library::start(
        &ui,
        store.clone(),
        tasks.handle().clone(),
        secrets,
        paths.data_dir().join("cache").join("pictures"),
    )?;

    let live = livetv::LiveTv::start(&ui, store.clone(), tasks.handle().clone())?;

    // On a computer whose graphics have no usable OpenGL, Slint draws the
    // window in software and the player cannot hook into it. Haven then
    // runs without the player (streams and channels are listed, not
    // playable) instead of not starting at all.
    #[cfg(feature = "player")]
    let player = match player::Player::start(&ui, store.clone()) {
        Ok(player) => Some(player),
        Err(err) => {
            eprintln!("haven: no player on this computer (no usable OpenGL): {err}");
            None
        }
    };
    #[cfg(feature = "player")]
    if let Some(player) = &player {
        let playing = player.clone();
        library.set_player(move |request| playing.play(request));
        let playing = player.clone();
        live.set_player(move |request| playing.play(request));
        let library = std::rc::Rc::downgrade(&library);
        player.on_closed(move |played| {
            if let Some(library) = library.upgrade() {
                library.player_closed(played);
            }
        });
    }

    #[cfg(debug_assertions)]
    let _devtools = devtools::start(&ui);

    ui.run()?;
    // Don't wait for downloads that are still running.
    tasks.shutdown_background();

    #[cfg(feature = "player")]
    {
        if let Some(player) = &player {
            player.leaving();
        }
        // Dropping the window tears down its renderer, which frees the
        // video's OpenGL objects while that is still possible. Only then
        // can mpv be ended.
        #[cfg(debug_assertions)]
        drop(_devtools);
        drop((library, live));
        drop(ui);
        if let Some(player) = &player {
            player.finish();
        }
    }
    #[cfg(not(feature = "player"))]
    drop((library, live));
    Ok(())
}

/// A failed save must not take the app down; the choice still holds for
/// this run and the reason is logged.
fn save(settings: &Settings, store: &Store) {
    if let Err(err) = settings.save(store) {
        eprintln!("haven: could not save settings: {err}");
    }
}

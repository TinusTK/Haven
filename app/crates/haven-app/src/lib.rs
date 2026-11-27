//! Haven's app: the window, its pages and what drives them. The desktop
//! program (`main.rs`) starts it with [`run`].

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
mod external;
mod library;
mod livetv;
#[cfg(feature = "player")]
mod player;
mod remote;
mod whats_new;

slint::include_modules!();

/// Where Haven keeps its data on a computer: the usual place for the
/// system, or, in a debug build, the folder `HAVEN_DATA_DIR` names.
pub fn desktop_paths() -> Result<AppPaths, Box<dyn Error>> {
    #[cfg(debug_assertions)]
    if let Some(folder) = std::env::var_os("HAVEN_DATA_DIR") {
        return Ok(AppPaths::at(folder)?);
    }
    Ok(AppPaths::discover()?)
}

/// Opens Haven's window and runs until it is closed. `phone` lays the
/// pages out for a phone's screen.
pub fn run(paths: AppPaths, phone: bool) -> Result<(), Box<dyn Error>> {
    haven_core::diag::start(&paths.data_dir().join("haven.log"));
    let store = Rc::new(Store::open(&paths.database())?);
    // Haven has run here before (not a fresh install): for "What's new".
    let existing = store.get::<serde_json::Value>("settings").ok().flatten().is_some();
    let settings = Rc::new(RefCell::new(Settings::load(&store)?));

    // Must run before the first window exists: the player draws video with
    // OpenGL, so the window has to be drawn with OpenGL too.
    // If that cannot be had, carry on: Haven then runs without the player.
    #[cfg(feature = "player")]
    if let Err(err) = slint::BackendSelector::new().require_opengl().select() {
        eprintln!("haven: OpenGL is not available: {err}");
    }

    let ui = AppWindow::new()?;
    // The big-screen layout with the remote control, for checks on a
    // computer: HAVEN_TV in a debug build.
    let tv = cfg!(debug_assertions) && std::env::var_os("HAVEN_TV").is_some();
    ui.set_tv(tv);
    remote::connect(&ui);
    // A store build: no add-ons.
    ui.set_addons_hidden(library::NO_ADDONS);
    ui.set_own_network_only(haven_servers::ONLY_OWN_NETWORK);
    ui.set_compact(phone);
    ui.global::<Theme>().set_phone(phone);
    ui.global::<Remote>().set_on(tv);
    if tv {
        ui.invoke_remote_start();
    }
    ui.set_version(concat!(env!("CARGO_PKG_VERSION"), " build ", env!("HAVEN_BUILD")).into());
    ui.set_data_path(paths.data_dir().display().to_string().into());
    ui.set_log_path(paths.data_dir().join("haven.log").display().to_string().into());
    ui.set_dark(settings.borrow().theme == ThemeChoice::Dark);
    let page_count = ui.get_pages().row_count() as u32;
    let last = settings.borrow().last_page.min(page_count.saturating_sub(1)) as i32;
    ui.set_page(last);

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

    // The loading screen: the halo and a bar that follows what Haven
    // loads at start. What was loaded last time is kept and shows at
    // once (then is brought up to date behind the scenes), so on most
    // starts the bar fills straight away. It shows for at least a second
    // (so it does not flash), else until the bar is full, and never more
    // than 12 seconds, so a slow or dead network never keeps Haven shut.
    let whats_new_due = whats_new::prepare(&ui, &store, existing);
    ui.on_whats_new_done({
        let (ui, store) = (ui.as_weak(), store.clone());
        move |dont_show_again| {
            whats_new::done(&store, dont_show_again);
            if let Some(ui) = ui.upgrade() {
                ui.set_whats_new_shown(false);
            }
        }
    });
    let splash = Rc::new(slint::Timer::default());
    {
        let started = std::time::Instant::now();
        let shown = std::cell::Cell::new(0.0f32);
        let (ui, library, timer) = (ui.as_weak(), library.clone(), Rc::downgrade(&splash));
        splash.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(80), move || {
            let Some(ui) = ui.upgrade() else { return };
            let elapsed = started.elapsed().as_secs_f32();
            let real = if elapsed >= 12.0 { 1.0 } else { library.startup().clamp(0.0, 1.0) };
            // In the first second the bar cannot run ahead of the clock.
            let target = if elapsed < 1.0 { real.min(elapsed) } else { real };
            shown.set(shown.get().max(target));
            ui.set_splash_progress(shown.get());
            if shown.get() >= 1.0 && elapsed >= 1.0 {
                haven_core::diag::note(&format!("loading screen closed after {elapsed:.1} s"));
                ui.set_splash_shown(false);
                let faded = ui.as_weak();
                slint::Timer::single_shot(std::time::Duration::from_millis(450), move || {
                    if let Some(ui) = faded.upgrade() {
                        ui.set_splash_gone(true);
                        if whats_new_due {
                            ui.set_whats_new_shown(true);
                        }
                    }
                });
                if let Some(timer) = timer.upgrade() {
                    timer.stop();
                }
            }
        });
    }

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
    // Videos play in Haven's own player, or in another program if the
    // user prefers one (Settings > Player) or Haven's own cannot run.
    #[cfg(feature = "player")]
    let own: Option<Rc<dyn Fn(library::PlayRequest)>> = player.clone().map(|playing| {
        Rc::new(move |request: library::PlayRequest| {
            playing.play(request)
        }) as Rc<dyn Fn(library::PlayRequest)>
    });
    #[cfg(not(feature = "player"))]
    let own: Option<Rc<dyn Fn(library::PlayRequest)>> = None;
    let external = external::External::start(&ui, store.clone(), own.is_some());
    let play = {
        let (external, own) = (external.clone(), own.clone());
        move |request: library::PlayRequest| match &own {
            Some(own) if !external.wanted() => {
                external.clear();
                own(request)
            }
            _ => external.play(&request),
        }
    };
    library.set_player(play.clone());
    live.set_player(play);
    #[cfg(feature = "player")]
    if let Some(player) = &player {
        let playing = player.clone();
        library.set_player_dismiss(move || playing.dismiss());
        let next = std::rc::Rc::downgrade(&library);
        player.on_next(move |played| next.upgrade().is_some_and(|library| library.play_next(played)));
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
    drop(splash);
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

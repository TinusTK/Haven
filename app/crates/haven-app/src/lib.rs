//! Haven's app: the window, its pages and what drives them. The desktop
//! program (`main.rs`) and the Android app (`android_main` below) both
//! start it with [`run`].

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

slint::include_modules!();

/// Where Haven keeps its data on a computer: the usual place for the
/// system, or, in a debug build, the folder `HAVEN_DATA_DIR` names.
pub fn desktop_paths() -> Result<AppPaths, Box<dyn Error>> {
    #[cfg(debug_assertions)]
    if let Some(folder) = std::env::var_os("HAVEN_DATA_DIR") {
        return Ok(AppPaths::at(folder)?);
    }
    // iOS: inside the app's own container, which the system names HOME.
    #[cfg(target_os = "ios")]
    if let Some(home) = std::env::var_os("HOME") {
        let folder = std::path::Path::new(&home).join("Library/Application Support/Haven");
        return Ok(AppPaths::at(folder)?);
    }
    Ok(AppPaths::discover()?)
}

/// True on an iPhone (or iPod touch): its pages are laid out for a
/// narrow screen. An iPad gets the layout a computer gets.
#[cfg(target_os = "ios")]
pub fn is_phone() -> bool {
    // The simulator says which device it plays through this variable;
    // a real device through the machine name ("iPhone16,1", "iPad14,3").
    let model = std::env::var("SIMULATOR_MODEL_IDENTIFIER").ok().or_else(|| {
        let mut name: libc::utsname = unsafe { std::mem::zeroed() };
        (unsafe { libc::uname(&mut name) } == 0).then(|| {
            unsafe { std::ffi::CStr::from_ptr(name.machine.as_ptr()) }.to_string_lossy().into_owned()
        })
    });
    !model.is_some_and(|model| model.starts_with("iPad"))
}

/// Opens Haven's window and runs until it is closed. `phone` lays the
/// pages out for a phone's screen.
pub fn run(paths: AppPaths, phone: bool) -> Result<(), Box<dyn Error>> {
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
    // A phone or tablet: no Add-ons page, and updates come as new APKs.
    ui.set_mobile(cfg!(any(target_os = "android", target_os = "ios")));
    ui.set_compact(phone);
    ui.set_ios(cfg!(target_os = "ios"));
    ui.set_version(env!("CARGO_PKG_VERSION").into());
    ui.set_data_path(paths.data_dir().display().to_string().into());
    ui.set_log_path(paths.data_dir().join("haven.log").display().to_string().into());
    ui.set_dark(settings.borrow().theme == ThemeChoice::Dark);
    let page_count = ui.get_pages().row_count() as u32;
    let last = settings.borrow().last_page.min(page_count.saturating_sub(1)) as i32;
    // A phone build has no Add-ons page to come back to.
    ui.set_page(if library::MOBILE && ui.get_pages().row_data(last as usize).is_some_and(|name| name == "Add-ons") { 0 } else { last });

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
    // Videos play in Haven's own player, or in another program if the
    // user prefers one (Settings > Player) or Haven's own cannot run.
    #[cfg(feature = "player")]
    let own: Option<Rc<dyn Fn(library::PlayRequest)>> = player.clone().map(|playing| {
        Rc::new(move |request| playing.play(request)) as Rc<dyn Fn(library::PlayRequest)>
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

/// Android's handle on the running app, for asking the system things.
#[cfg(target_os = "android")]
static ANDROID: std::sync::OnceLock<slint::android::AndroidApp> = std::sync::OnceLock::new();

/// Android starts Haven here.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    let data = app.internal_data_path();
    let _ = ANDROID.set(app.clone());
    // Under 600 dp across its short side a screen is a phone's; a
    // tablet has room for the layout a computer gets.
    let phone = app.config().smallest_screen_width_dp().is_none_or(|across| across < 600);
    if let Err(err) = slint::android::init(app) {
        eprintln!("haven: could not start on Android: {err}");
        return;
    }
    let paths = match data.map(AppPaths::at) {
        Some(Ok(paths)) => paths,
        _ => return eprintln!("haven: Android gave no folder for Haven's data"),
    };
    if let Err(err) = run(paths, phone) {
        eprintln!("haven: {err}");
    }
}

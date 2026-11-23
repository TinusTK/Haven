//! Add-ons and everything they fill: the Home shelves, search results, a
//! title's page with its episodes, and its streams. Network work runs off
//! the UI thread and comes back through [`on_ui`].

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, atomic::AtomicU64},
};

#[cfg(feature = "addons")]
mod addons;
mod backup;
mod calendar;
mod cast;
mod downloads;
mod explore;
mod kept;
mod local;
mod marks;
mod home;
mod offers;
mod ondemand;
mod servers;
mod sources;
mod sync;
mod tracking;
mod updates;

use haven_titles::{Catalog, Catalogues, MetaDetail, MetaPreview, Source, Video};
use haven_core::{
    http,
    storage::{Saved, Store},
};
use haven_metadata::{List, Tmdb};
use haven_player::{clock, resume};
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use tokio::{runtime::Handle, sync::Semaphore};

use crate::{AppWindow, Detail, EpisodeRow, Poster, Shelf, StreamRow};

/// Titles on a media server library's A to Z shelf on Home; Explore
/// shows the whole library.
const SERVER_SHELF_LENGTH: usize = 60;

/// Titles shown per shelf (a catalogue's first page can be far longer).
const SHELF_LENGTH: usize = 30;
const IMAGE_BYTES: usize = 6 * 1024 * 1024;
/// Posters are kept at twice the card size, for sharp 2x screens.
const POSTER_SIZE: (u32, u32) = (300, 450);
const BACKGROUND_SIZE: (u32, u32) = (1600, 900);
const IMAGES_AT_ONCE: usize = 6;
/// Shelves asked of media servers at the same time. A server that
/// gathers add-on catalogues passes every ask on to the add-on, and an
/// add-on asked for dozens of lists at once refuses them all (and then
/// refuses the streams too).
const SERVER_SHELVES_AT_ONCE: usize = 2;
const CAST_SHOWN: usize = 6;
pub(super) const CONTINUE_LENGTH: u32 = 20;

type Pixels = SharedPixelBuffer<Rgba8Pixel>;

thread_local! {
    /// Lets finished background work find the library again on the UI thread.
    static CURRENT: RefCell<Option<Rc<Library>>> = const { RefCell::new(None) };
}

/// Runs `work` on the UI thread with the library, from any thread.
fn on_ui(work: impl FnOnce(&Rc<Library>) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        let library = CURRENT.with(|current| current.borrow().clone());
        if let Some(library) = library {
            work(&library);
        }
    });
}

#[cfg(all(feature = "store", feature = "addons"))]
compile_error!("a store build has no add-ons: build it with --no-default-features --features store");

/// Picks the words for this build: `$store` where there are no add-ons,
/// `$addons` where there are. Only the chosen words go into the program.
macro_rules! words {
    ($store:expr, $addons:expr) => {{
        #[cfg(not(feature = "addons"))]
        let chosen = $store;
        #[cfg(feature = "addons")]
        let chosen = $addons;
        chosen
    }};
}
pub(crate) use words;

/// The two places that show shelves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Board {
    Home,
    Search,
    /// The media servers' shelves on Home.
    Servers,
    /// The built-in TMDB catalogue's shelves on Home.
    Catalog,
}

/// One board's shelves and what is needed to fill them in later.
#[derive(Default)]
struct ShelfSet {
    shelves: Rc<VecModel<Shelf>>,
    /// The poster model of each shelf, in shelf order.
    posters: RefCell<Vec<Rc<VecModel<Poster>>>>,
    /// Goes up whenever the shelves are rebuilt, so late answers for the
    /// old shelves are dropped.
    generation: Cell<u64>,
    /// Pictures already on screen, by title id, so rebuilt shelves show
    /// them at once instead of loading them again.
    images: RefCell<std::collections::HashMap<String, Image>>,
    /// Each shelf's name for its kept copy (empty: not kept).
    kept_names: RefCell<Vec<String>>,
}

/// A shelf to build: which catalogue of which add-on, and its heading.
struct ShelfPlan {
    title: String,
    source: String,
    from: ShelfFrom,
}

/// Where a shelf's titles come from.
enum ShelfFrom {
    /// A catalogue of a source (the built-in one or an add-on), by the
    /// source's address.
    Catalogue { url: String, catalog: Catalog },
    /// A library on a media server: its newest titles, or all by name.
    Server { server: haven_servers::Server, library: String, latest: bool },
    /// A media server's answer to the search words.
    ServerSearch { server: haven_servers::Server },
    /// One of TMDB's ready-made lists of films or series.
    Tmdb { list: List, series: bool },
    /// TMDB's answer to the search words.
    TmdbSearch { series: bool },
}

/// What the player reports when it closes.
#[cfg_attr(not(feature = "player"), allow(dead_code))]
pub struct Played {
    pub kind: String,
    pub title_id: String,
    pub video_id: String,
    pub position: f64,
    pub duration: f64,
    pub finished: bool,
}

/// The title whose page is open.
struct OpenTitle {
    kind: String,
    id: String,
    meta: Option<MetaDetail>,
    /// The episode to pick once the details arrive (from Continue Watching).
    wanted: Option<String>,
}

/// The film or episode whose streams are listed.
struct Listed {
    video_id: String,
    /// For an episode, which one ("S1 E2 · Name"); empty for a film.
    detail: String,
}

/// Everything the player needs to play a stream and remember the place.
#[cfg_attr(not(feature = "player"), allow(dead_code))]
pub struct PlayRequest {
    pub address: String,
    pub title: String,
    pub detail: String,
    pub kind: String,
    pub title_id: String,
    pub video_id: String,
    pub poster: Option<String>,
    /// Remember how far it was watched (films and episodes, not channels).
    pub remember: bool,
    /// Where it plays from, in words, for the player's information.
    pub source: String,
    /// The episode after this one, in words; `None` if there is none.
    pub next: Option<String>,
}

#[derive(Clone, Copy)]
enum DetailImage {
    Poster,
    Background,
}

/// Holds back work that only Home needs while Home is not showing, so
/// it does not load the line or the servers for nothing.
#[derive(Default)]
struct Pause {
    held: std::sync::atomic::AtomicBool,
    wake: tokio::sync::Notify,
}

impl Pause {
    fn set(&self, held: bool) {
        self.held.store(held, std::sync::atomic::Ordering::Relaxed);
        if !held {
            self.wake.notify_waiters();
        }
    }

    /// Returns once the work may go on.
    async fn pass(&self) {
        loop {
            let woken = self.wake.notified();
            if !self.held.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            // Checked again now and then, in case a wake was missed.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), woken).await;
        }
    }
}

/// Said when nothing knows the title that was opened.
const NO_DETAILS: &str =
    words!("There are no details for this title.", "None of your add-ons has details for this title.");

/// A phone or tablet build (updates come as a new APK or IPA).
pub(crate) const MOBILE: bool = cfg!(any(target_os = "android", target_os = "ios"));

/// A build without add-ons (the App Store and Google Play builds): no
/// add-on code, no Add-ons page, never a word about add-ons. Every other
/// build, the phone ones on GitHub too, has add-ons.
pub(crate) const NO_ADDONS: bool = cfg!(not(feature = "addons"));

/// Where the built-in catalogue's list of catalogues is kept between runs.
const CINEMETA_KEPT: &str = "cinemeta_description";

/// A counter that says which list of places to watch is current. Work
/// off the UI thread reads it to stop when its list is no longer wanted.
#[derive(Default)]
struct Epoch(Arc<std::sync::atomic::AtomicU64>);

impl Epoch {
    fn get(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn set(&self, value: u64) {
        self.0.store(value, std::sync::atomic::Ordering::Relaxed);
    }
    fn shared(&self) -> Arc<std::sync::atomic::AtomicU64> {
        self.0.clone()
    }
}

pub struct Library {
    ui: slint::Weak<AppWindow>,
    store: Rc<Store>,
    tasks: Handle,
    /// Reads catalogues and details: the built-in catalogue's, and the
    /// add-ons' where there are add-ons.
    catalogues: Catalogues,
    #[cfg(feature = "addons")]
    addons: haven_addons::Client,
    http: reqwest::Client,
    /// Every source of catalogues and details in use: the built-in
    /// catalogue first, then the add-ons that are switched on.
    installed: RefCell<Vec<Source>>,
    /// The built-in catalogue.
    builtin: RefCell<Source>,
    /// The add-ons that are switched on, as sources.
    #[cfg(feature = "addons")]
    addon_sources: RefCell<Vec<Source>>,
    /// The address of every installed add-on, switched on or not.
    #[cfg(feature = "addons")]
    addon_urls: RefCell<Vec<String>>,
    home: ShelfSet,
    search: ShelfSet,
    image_gate: Arc<Semaphore>,
    server_gate: Arc<Semaphore>,
    calendar: calendar::Calendar,
    downloads: downloads::Downloads,
    home_top: home::HomeTop,
    sync: sync::SyncState,
    backup: backup::BackupState,
    local: local::LocalState,
    updates: updates::Updates,
    /// Held while Home is not the page showing.
    home_pause: Arc<Pause>,
    /// Where downloaded pictures are kept between runs.
    picture_cache: Arc<std::path::PathBuf>,
    /// Shelf lists and calendar dates asked for and not answered yet
    /// (a pulled-down Home turns its sign until none are left).
    fetches_left: Cell<usize>,

    open: RefCell<Option<OpenTitle>>,
    /// Goes up when another title is opened or the page is closed.
    detail_generation: Cell<u64>,
    seasons: Rc<VecModel<i32>>,
    episodes: Rc<VecModel<EpisodeRow>>,
    streams: Rc<VecModel<StreamRow>>,
    /// Goes up whenever streams are asked for again.
    stream_generation: Epoch,
    /// How many places to watch show per provider, where more than ten.
    stream_limits: RefCell<std::collections::HashMap<String, i32>>,
    /// Add-ons that have not answered the current stream request yet.
    streams_pending: Cell<usize>,
    /// What is behind each row of `streams`.
    stream_sources: RefCell<Vec<sources::StreamSource>>,
    /// How many of the first rows of `streams` come from media servers
    /// (they are kept above the add-ons' rows).
    server_rows: Cell<usize>,
    /// The media server item (`srv:…`) that is playing, if what plays
    /// came from a media server.
    playing_item: RefCell<Option<String>>,
    listed: RefCell<Option<Listed>>,
    /// Plays a stream; unset in a build without the player.
    player: RefCell<Option<Box<dyn Fn(PlayRequest)>>>,
    /// Leaves the player page when the next episode cannot start by itself.
    player_dismiss: RefCell<Option<Box<dyn Fn()>>>,
    /// The source and name of the row that was last played, to pick the
    /// same again for the next episode.
    last_played: RefCell<Option<(String, String)>>,
    /// Set while the next episode's places to watch are being asked for
    /// and the one like the last is to be played as soon as it shows.
    autoplay: RefCell<Option<(String, String)>>,
    continuing: Rc<VecModel<Poster>>,
    continue_generation: Cell<u64>,
    watchlist: Rc<VecModel<Poster>>,
    watchlist_generation: Cell<u64>,
    /// Everything watched, newest first (the Library's History).
    history: Rc<VecModel<Poster>>,
    history_generation: Cell<u64>,

    /// The media servers in use (those switched on).
    servers: RefCell<Vec<haven_servers::Server>>,
    /// Every media server signed in to, switched on or not.
    all_servers: RefCell<Vec<haven_servers::Server>>,
    server_client: haven_servers::Client,
    server_shelves: ShelfSet,
    server_generation: Cell<u64>,
    /// Whether each server answers: 0 not known yet, 1 online, 2 offline.
    server_states: RefCell<std::collections::HashMap<i64, i32>>,
    /// The Xtream Codes accounts' films and series, as far as fetched.
    on_demand: ondemand::Lists,
    /// The add-ons that media servers use and share with their owner.
    #[cfg(feature = "addons")]
    shared_addons: RefCell<Vec<(String, haven_servers::SharedAddon)>>,
    /// Every library of every server that answered, for Explore.
    server_libraries: RefCell<Vec<(haven_servers::Server, haven_servers::Library)>>,
    /// Asks the servers now and then whether they still answer.
    server_timer: slint::Timer,
    /// The TMDB catalogue's shelves on Home.
    catalog: ShelfSet,
    explore: explore::Explore,
    /// Goes up when a Plex sign-in starts or is cancelled.
    plex_attempt: Arc<AtomicU64>,

    /// Keys from the secrets file, under the ones typed into Settings.
    secrets_file: haven_core::secrets::Secrets,
    /// The keys in use right now.
    keys: RefCell<haven_core::secrets::Secrets>,
    /// Trakt and Simkl sign-ins.
    accounts: RefCell<Vec<haven_tracking::Account>>,
    /// Goes up when a tracking sign-in starts or is cancelled.
    tracking_attempt: Arc<AtomicU64>,

    /// The Chromecasts found by the last search.
    cast_devices: RefCell<Vec<haven_cast::Device>>,
    cast_session: RefCell<Option<haven_cast::Session>>,
    /// Goes up with each new casting session.
    cast_generation: Cell<u64>,
}

impl Library {
    /// Loads the installed add-ons, hooks up the UI and starts filling Home.
    pub fn start(
        ui: &AppWindow,
        store: Rc<Store>,
        tasks: Handle,
        secrets_file: haven_core::secrets::Secrets,
        picture_cache: std::path::PathBuf,
    ) -> Result<Rc<Self>, http::HttpError> {
        let http = http::client()?;
        let server_client = haven_servers::Client::new(http.clone(), device_id(&store));
        let library = Rc::new(Self {
            ui: ui.as_weak(),
            store,
            tasks,
            catalogues: Catalogues::new(http.clone()),
            #[cfg(feature = "addons")]
            addons: haven_addons::Client::new(http.clone()),
            http,
            installed: RefCell::default(),
            builtin: RefCell::new(Source::cinemeta(None)),
            #[cfg(feature = "addons")]
            addon_sources: RefCell::default(),
            #[cfg(feature = "addons")]
            addon_urls: RefCell::default(),
            home: ShelfSet::default(),
            search: ShelfSet::default(),
            image_gate: Arc::new(Semaphore::new(IMAGES_AT_ONCE)),
            server_gate: Arc::new(Semaphore::new(SERVER_SHELVES_AT_ONCE)),
            home_pause: Arc::default(),
            calendar: calendar::Calendar::default(),
            downloads: downloads::Downloads::default(),
            home_top: home::HomeTop::default(),
            sync: sync::SyncState::default(),
            backup: backup::BackupState::default(),
            local: local::LocalState::default(),
            updates: updates::Updates::default(),
            picture_cache: Arc::new(picture_cache),
            fetches_left: Cell::new(0),
            open: RefCell::default(),
            detail_generation: Cell::new(0),
            seasons: Rc::default(),
            episodes: Rc::default(),
            streams: Rc::default(),
            stream_generation: Epoch::default(),
            stream_limits: RefCell::default(),
            streams_pending: Cell::new(0),
            stream_sources: RefCell::default(),
            server_rows: Cell::new(0),
            playing_item: RefCell::default(),
            listed: RefCell::default(),
            player: RefCell::default(),
            player_dismiss: RefCell::default(),
            last_played: RefCell::default(),
            autoplay: RefCell::default(),
            continuing: Rc::default(),
            continue_generation: Cell::new(0),
            watchlist: Rc::default(),
            watchlist_generation: Cell::new(0),
            history: Rc::default(),
            history_generation: Cell::new(0),
            servers: RefCell::default(),
            all_servers: RefCell::default(),
            server_client,
            server_shelves: ShelfSet::default(),
            server_generation: Cell::new(0),
            server_states: RefCell::default(),
            on_demand: ondemand::Lists::default(),
            #[cfg(feature = "addons")]
            shared_addons: RefCell::default(),
            server_libraries: RefCell::default(),
            server_timer: slint::Timer::default(),
            catalog: ShelfSet::default(),
            explore: explore::Explore::default(),
            plex_attempt: Arc::default(),
            secrets_file,
            keys: RefCell::default(),
            accounts: RefCell::default(),
            tracking_attempt: Arc::default(),
            cast_devices: RefCell::default(),
            cast_session: RefCell::default(),
            cast_generation: Cell::new(0),
        });
        CURRENT.with(|current| *current.borrow_mut() = Some(library.clone()));

        ui.set_shelves(ModelRc::from(library.home.shelves.clone()));
        ui.set_search_shelves(ModelRc::from(library.search.shelves.clone()));
        ui.set_catalog_shelves(ModelRc::from(library.catalog.shelves.clone()));
        ui.set_seasons(ModelRc::from(library.seasons.clone()));
        ui.set_episodes(ModelRc::from(library.episodes.clone()));
        ui.set_streams(ModelRc::from(library.streams.clone()));
        ui.set_watchlist(ModelRc::from(library.watchlist.clone()));
        ui.set_continue_shelf(Shelf {
            ranked: false,
            title: "Continue Watching".into(),
            source: "".into(),
            loading: false,
            error: "".into(),
            items: ModelRc::from(library.continuing.clone()),
        });

        // Each handler holds the library; the library only holds the
        // window weakly, so nothing keeps the other alive forever.
        macro_rules! connect {
            ($on:ident, |$($arg:ident),*| $call:expr) => {
                ui.$on({
                    let library = library.clone();
                    move |$($arg),*| {
                        let library = &library;
                        $call(library)
                    }
                });
            };
        }
        #[cfg(feature = "addons")]
        library.connect_addons(ui);
        connect!(on_search, |query| |l: &Rc<Self>| l.run_search(query.as_str()));
        connect!(on_open_title, |poster| |l: &Rc<Self>| l.open_title(poster));
        connect!(on_close_detail, | | |l: &Rc<Self>| l.close_detail());
        connect!(on_season_selected, |season| |l: &Rc<Self>| l.show_season(season));
        connect!(on_episode_selected, |id| |l: &Rc<Self>| l.open_episode(id.as_str()));
        connect!(on_filter_episodes, |words| |l: &Rc<Self>| l.filter_episodes(words.as_str()));
        connect!(on_play_stream, |row| |l: &Rc<Self>| l.play_stream(row));
        connect!(on_home_showing_changed, |showing| |l: &Rc<Self>| l.home_pause.set(!showing));
        connect!(on_show_more_streams, |group| |l: &Rc<Self>| l.show_more_streams(group.as_str()));
        connect!(on_toggle_watchlist, | | |l: &Rc<Self>| l.toggle_watchlist());
        connect!(on_clear_cache, | | |l: &Rc<Self>| l.clear_cache());
        connect!(on_home_refresh, | | |l: &Rc<Self>| l.refresh_home());
        connect!(on_show_sources, | | |l: &Rc<Self>| l.show_film_sources());
        connect!(on_explore_refresh, | | |l: &Rc<Self>| l.refresh_explore());
        connect!(on_remove_continue, |poster| |l: &Rc<Self>| l.remove_continue(&poster));
        library.show_cache_size();

        library.home_pause.set(!ui.get_home_showing());
        library.load_builtin();
        #[cfg(feature = "addons")]
        library.load_installed();
        #[cfg(not(feature = "addons"))]
        library.load_sources();
        library.refresh_builtin();
        library.reload_home();
        library.reload_continue();
        library.reload_watchlist();
        library.start_tracking(ui);
        library.start_sync(ui);
        library.start_explore(ui);
        library.start_offers(ui);
        library.start_home_top(ui);
        library.start_calendar(ui);
        library.start_marks(ui);
        library.start_downloads(ui);
        library.start_history(ui);
        library.start_updates(ui);
        library.start_servers(ui);
        library.start_backup(ui);
        library.start_local(ui);
        library.start_casting(ui);
        library.reload_catalog();
        Ok(library)
    }

    /// How far start-up loading has got, 0 to 1 (the loading screen).
    pub fn startup(&self) -> f32 {
        self.startup_progress()
    }

    /// Gives the library a player; from then on Direct streams can be
    /// played. Without one they are only listed.
    #[cfg_attr(not(feature = "player"), allow(dead_code))]
    pub fn set_player(&self, play: impl Fn(PlayRequest) + 'static) {
        *self.player.borrow_mut() = Some(Box::new(play));
    }

    /// Gives the library the way out of the player page.
    #[cfg_attr(not(feature = "player"), allow(dead_code))]
    pub fn set_player_dismiss(&self, dismiss: impl Fn() + 'static) {
        *self.player_dismiss.borrow_mut() = Some(Box::new(dismiss));
    }

    /// Call when the player has closed: what was watched may have changed.
    #[cfg_attr(not(feature = "player"), allow(dead_code))]
    pub fn player_closed(&self, played: Option<Played>) {
        if let Some(played) = played {
            self.report_played(&played);
            if played.finished {
                self.track_watched(&played.kind, &played.title_id, &played.video_id);
            }
        }
        *self.autoplay.borrow_mut() = None;
        self.reload_continue();
        self.show_resume_note();
        self.refresh_ticks();
    }

    /// Puts the gold ticks right after something was watched: the open
    /// title's episodes (or the film itself) and the calendar.
    pub(super) fn refresh_ticks(&self) {
        let open = self.open.borrow().as_ref().map(|open| (open.kind.clone(), open.id.clone()));
        if let Some((kind, id)) = open {
            let film = self.film_watched(&kind, &id);
            self.update_detail(|detail| detail.watched = film);
            let finished = self.finished_here();
            for at in 0..self.episodes.row_count() {
                if let Some(mut row) = self.episodes.row_data(at) {
                    let now = finished.contains(row.id.as_str());
                    if row.watched != now {
                        row.watched = now;
                        self.episodes.set_row_data(at, row);
                    }
                }
            }
            self.update_season_flag();
        }
        self.redraw_calendar();
    }

    /// Whether every episode listed for the season is watched (the
    /// season button then offers to mark it unwatched).
    fn update_season_flag(&self) {
        let rows = self.episodes.row_count();
        let all = rows > 0 && self.episodes.iter().all(|row| row.watched);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_season_all_watched(all);
        }
    }

    /// Films' ticks on Home's shelves and in Explore, after a mark.
    pub(super) fn refresh_shelf_ticks(&self) {
        let fix = |items: &VecModel<Poster>| {
            for at in 0..items.row_count() {
                if let Some(mut poster) = items.row_data(at) {
                    let now = self.film_watched(poster.kind.as_str(), poster.id.as_str());
                    if poster.watched != now {
                        poster.watched = now;
                        items.set_row_data(at, poster);
                    }
                }
            }
        };
        for set in [&self.home, &self.server_shelves, &self.catalog, &self.search] {
            for items in set.posters.borrow().iter() {
                fix(items);
            }
        }
        fix(&self.explore_items());
    }

    fn board(&self, board: Board) -> &ShelfSet {
        match board {
            Board::Home => &self.home,
            Board::Search => &self.search,
            Board::Servers => &self.server_shelves,
            Board::Catalog => &self.catalog,
        }
    }

    // ---- Sources of catalogues and details ----------------------------

    /// The built-in catalogue as it was last described, if it was.
    fn load_builtin(&self) {
        let kept: Option<String> = self.store.get(CINEMETA_KEPT).ok().flatten();
        if let Some(catalogs) = kept.and_then(|json| haven_titles::parse_catalogs(&json).ok()) {
            if !catalogs.is_empty() {
                *self.builtin.borrow_mut() = Source::cinemeta(Some(catalogs));
            }
        }
    }

    /// Puts together the sources in use: the built-in catalogue first.
    pub(super) fn load_sources(&self) {
        #[cfg_attr(not(feature = "addons"), allow(unused_mut))]
        let mut sources = vec![self.builtin.borrow().clone()];
        #[cfg(feature = "addons")]
        sources.extend(self.addon_sources.borrow().iter().cloned());
        *self.installed.borrow_mut() = sources;
    }

    /// Asks the built-in catalogue which catalogues it has now; when that
    /// changed, keeps it and rebuilds what shows them.
    fn refresh_builtin(&self) {
        let catalogues = self.catalogues.clone();
        self.tasks.spawn(async move {
            let Ok(json) = catalogues.description_json(haven_titles::CINEMETA_URL).await else { return };
            let Ok(catalogs) = haven_titles::parse_catalogs(&json) else { return };
            if catalogs.is_empty() {
                return;
            }
            on_ui(move |library| {
                let fresh = Source::cinemeta(Some(catalogs));
                if *library.builtin.borrow() == fresh {
                    return;
                }
                if let Err(err) = library.store.put(CINEMETA_KEPT, &json) {
                    eprintln!("haven: could not keep the catalogue list: {err}");
                }
                *library.builtin.borrow_mut() = fresh;
                library.load_sources();
                library.reload_home();
                library.rebuild_explore();
            });
        });
    }

    // ---- Shelves (Home and Search) ------------------------------------

    /// Rebuilds Home: one shelf per catalogue that needs no input.
    fn reload_home(&self) {
        let plans = self
            .installed
            .borrow()
            .iter()
            .flat_map(|addon| {
                addon.catalogs.iter().filter(|c| !c.needs_input()).map(|catalog| {
                    ShelfPlan {
                        title: catalog.title(),
                        source: addon.name.clone(),
                        from: ShelfFrom::Catalogue { url: addon.url.clone(), catalog: catalog.clone() },
                    }
                })
            })
            .collect();
        self.fill(Board::Home, plans, None);
    }

    /// The built-in catalogue, if a TMDB key is set.
    pub(super) fn tmdb(&self) -> Option<Tmdb> {
        let key = self.keys.borrow().tmdb_api_key.clone()?;
        let tmdb = Tmdb::new(self.http.clone(), &key);
        // A stand-in for TMDB, to check Haven without a key.
        #[cfg(debug_assertions)]
        if let (Ok(api), Ok(images)) =
            (std::env::var("HAVEN_TMDB_API"), std::env::var("HAVEN_TMDB_IMAGES"))
        {
            return Some(tmdb.at(&api, &images));
        }
        Some(tmdb)
    }

    /// Rebuilds the TMDB shelves on Home; none without a TMDB key.
    pub(super) fn reload_catalog(&self) {
        let plans = if self.tmdb().is_some() {
            [
                ("Trending films", List::Trending, false),
                ("Trending series", List::Trending, true),
                ("In cinemas now", List::Now, false),
                ("On television now", List::Now, true),
                ("Popular films", List::Popular, false),
                ("Popular series", List::Popular, true),
                ("Coming soon", List::Upcoming, false),
                ("Top rated films", List::TopRated, false),
                ("Top rated series", List::TopRated, true),
            ]
            .into_iter()
            .map(|(title, list, series)| ShelfPlan {
                title: title.to_owned(),
                source: "TMDB".to_owned(),
                from: ShelfFrom::Tmdb { list, series },
            })
            .collect()
        } else {
            Vec::new()
        };
        self.fill(Board::Catalog, plans, None);
    }

    fn clear_search(&self) {
        self.fill(Board::Search, Vec::new(), None);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_search_note("".into());
        }
    }

    /// Asks every searchable catalogue for `query`; each answers as a shelf.
    fn run_search(&self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return self.clear_search();
        }
        let plans: Vec<ShelfPlan> = self
            .installed
            .borrow()
            .iter()
            .flat_map(|addon| {
                addon.catalogs.iter().filter(|c| c.searchable()).map(|catalog| {
                    ShelfPlan {
                        title: catalog.results_title(),
                        source: addon.name.clone(),
                        from: ShelfFrom::Catalogue { url: addon.url.clone(), catalog: catalog.clone() },
                    }
                })
            })
            .collect();
        let mut plans = plans;
        plans.extend(self.servers.borrow().iter().map(|server| ShelfPlan {
            title: format!("Results on {}", server.name),
            source: server.kind.label().to_owned(),
            from: ShelfFrom::ServerSearch { server: server.clone() },
        }));
        if self.tmdb().is_some() {
            for (title, series) in [("Films", false), ("Series", true)] {
                plans.push(ShelfPlan {
                    title: title.to_owned(),
                    source: "TMDB".to_owned(),
                    from: ShelfFrom::TmdbSearch { series },
                });
            }
        }
        let note = if self.installed.borrow().is_empty() && plans.is_empty() {
            words!(
                "Add a media server or a TMDB key in Settings: search looks through those.",
                "Add a media server or a TMDB key in Settings, or install an add-on: search looks through those."
            )
        } else if plans.is_empty() {
            words!("Nothing you have added can be searched.", "None of your add-ons can be searched.")
        } else {
            ""
        };
        if let Some(ui) = self.ui.upgrade() {
            ui.set_search_note(note.into());
        }
        self.fill(Board::Search, plans, Some(query.to_owned()));
    }

    /// Puts the planned shelves on a board, each loading, and fetches them.
    fn fill(&self, board: Board, plans: Vec<ShelfPlan>, search: Option<String>) {
        let set = self.board(board);
        let generation = set.generation.get() + 1;
        set.generation.set(generation);
        // Pictures on the old shelves stay on hand for the new ones.
        {
            let mut images = set.images.borrow_mut();
            for items in set.posters.borrow().iter() {
                for poster in items.iter().filter(|poster| poster.has_image) {
                    images.insert(poster.id.to_string(), poster.image.clone());
                }
            }
        }

        let mut shelves = Vec::new();
        let mut posters = Vec::new();
        let mut kept_names = Vec::new();
        let mut kept_lists = Vec::new();
        for (shelf, plan) in plans.into_iter().enumerate() {
            // What this shelf held last time, shown until the fresh list comes.
            let kept_name = if board == Board::Search {
                String::new()
            } else {
                format!("shelf|{board:?}|{}|{}", plan.title, plan.source)
            };
            if !kept_name.is_empty() {
                if let Some(list) = self.kept::<Vec<MetaPreview>>(&kept_name) {
                    kept_lists.push((shelf, list));
                }
            }
            kept_names.push(kept_name);
            let items = Rc::new(VecModel::<Poster>::default());
            // Popular and trending lists show each title's place.
            let ranked = ["Popular", "Trending", "Top"].iter().any(|word| plan.title.starts_with(word));
            shelves.push(Shelf {
                ranked,
                title: plan.title.into(),
                source: plan.source.into(),
                loading: true,
                error: "".into(),
                items: ModelRc::from(items.clone()),
            });
            posters.push(items);

            let (addons, servers, search) =
                (self.catalogues.clone(), self.server_client.clone(), search.clone());
            let tmdb = self.tmdb();
            let gate = self.server_gate.clone();
            // Home's shelves wait while another page is showing.
            let pause = (board != Board::Search).then(|| self.home_pause.clone());
            let counted = board != Board::Search;
            if counted {
                self.fetches_left.set(self.fetches_left.get() + 1);
            }
            self.tasks.spawn(async move {
                let no_key = || "there is no TMDB key".to_owned();
                if let Some(pause) = &pause {
                    pause.pass().await;
                }
                // A media server's shelves take turns.
                let _turn = match &plan.from {
                    ShelfFrom::Server { .. } => gate.acquire_owned().await.ok(),
                    _ => None,
                };
                let metas = match &plan.from {
                    ShelfFrom::Catalogue { url, catalog } => addons
                        .catalog(url, catalog, search.as_deref())
                        .await
                        .map_err(|err| err.to_string()),
                    ShelfFrom::Server { server, library, latest: true } => {
                        servers.latest(server, library).await.map_err(|err| err.to_string())
                    }
                    ShelfFrom::Server { server, library, latest: false } => servers
                        .page(server, library, None, 0, SERVER_SHELF_LENGTH as u32)
                        .await
                        .map_err(|err| err.to_string()),
                    ShelfFrom::Tmdb { list, series } => match &tmdb {
                        Some(tmdb) => tmdb.list(*list, *series, 1).await.map_err(|err| err.to_string()),
                        None => Err(no_key()),
                    },
                    ShelfFrom::TmdbSearch { series } => match &tmdb {
                        Some(tmdb) => tmdb
                            .search(*series, search.as_deref().unwrap_or_default())
                            .await
                            .map_err(|err| err.to_string()),
                        None => Err(no_key()),
                    },
                    ShelfFrom::ServerSearch { server } => servers
                        .search(server, search.as_deref().unwrap_or_default())
                        .await
                        .map_err(|err| err.to_string()),
                };
                on_ui(move |library| {
                    if counted {
                        library.fetches_left.set(library.fetches_left.get().saturating_sub(1));
                    }
                    library.shelf_loaded(board, generation, shelf, metas)
                });
            });
        }
        *set.posters.borrow_mut() = posters;
        *set.kept_names.borrow_mut() = kept_names;
        set.shelves.set_vec(shelves);
        for (shelf, list) in kept_lists {
            self.shelf_loaded_from(board, generation, shelf, Ok(list), true);
        }
    }

    fn shelf_loaded(
        &self,
        board: Board,
        generation: u64,
        shelf: usize,
        metas: Result<Vec<MetaPreview>, String>,
    ) {
        self.shelf_loaded_from(board, generation, shelf, metas, false)
    }

    /// Fills a shelf, from a fresh answer or (`from_kept`) from what it
    /// held last time. A fresh answer replaces the kept one, unless it
    /// failed or holds the same titles.
    fn shelf_loaded_from(
        &self,
        board: Board,
        generation: u64,
        shelf: usize,
        metas: Result<Vec<MetaPreview>, String>,
        from_kept: bool,
    ) {
        let set = self.board(board);
        if generation != set.generation.get() {
            return;
        }
        let (Some(mut row), Some(items)) =
            (set.shelves.row_data(shelf), set.posters.borrow().get(shelf).cloned())
        else {
            return;
        };
        let kept_name = set.kept_names.borrow().get(shelf).cloned().unwrap_or_default();
        let showing_kept = !from_kept && !row.loading && items.row_count() > 0;
        row.loading = false;
        match metas {
            // The kept titles stay when the fresh ones cannot be had.
            Err(_) if showing_kept => {}
            Err(err) => row.error = format!("Could not load this: {err}.").into(),
            Ok(metas) => {
                // A shelf shows the start of a list; Explore shows it all.
                let length = if board == Board::Servers { SERVER_SHELF_LENGTH } else { SHELF_LENGTH };
                // A server that gathers catalogues lists the same film
                // more than once: show each once.
                let metas: Vec<MetaPreview> =
                    haven_titles::without_repeats(metas).into_iter().take(length).collect();
                if !from_kept && !kept_name.is_empty() {
                    self.keep(kept_name, metas.clone());
                }
                let same = showing_kept
                    && items.row_count() == metas.len()
                    && items.iter().zip(&metas).all(|(old, new)| old.id.as_str() == new.id && old.title.as_str() == new.name);
                if same {
                    row.error = "".into();
                    set.shelves.set_row_data(shelf, row);
                    return;
                }
                if board != Board::Search {
                    self.offer_hero(&metas);
                }
                let images = set.images.borrow();
                let mut on_hand = items.iter().filter(|p| p.has_image).map(|p| (p.id.to_string(), p.image.clone())).collect::<std::collections::HashMap<_, _>>();
                on_hand.extend(images.iter().map(|(id, image)| (id.clone(), image.clone())));
                drop(images);
                row.error = "".into();
                items.set_vec(
                    metas
                        .iter()
                        .map(|meta| Poster {
                            id: meta.id.as_str().into(),
                            kind: meta.kind.as_str().into(),
                            title: meta.name.as_str().into(),
                            year: meta.release_info.as_deref().unwrap_or_default().into(),
                            image: on_hand.get(&meta.id).cloned().unwrap_or_default(),
                            has_image: on_hand.contains_key(&meta.id),
                            video: "".into(),
                            progress: 0.0,
                            watched: self.film_watched(&meta.kind, &meta.id),
                            rating: meta.rating.as_deref().unwrap_or_default().into(),
                            rank: 0,
                        })
                        .collect::<Vec<_>>(),
                );
                for (item, meta) in metas.into_iter().enumerate() {
                    if on_hand.contains_key(&meta.id) {
                        continue;
                    }
                    let Some(url) = meta.poster else { continue };
                    let id = meta.id;
                    let pause = (board != Board::Search).then(|| self.home_pause.clone());
                    self.fetch_image_for(pause, url, POSTER_SIZE, move |library, pixels| {
                        library.poster_loaded(board, generation, shelf, item, &id, pixels);
                    });
                }
            }
        }
        set.shelves.set_row_data(shelf, row);
    }

    fn poster_loaded(&self, board: Board, generation: u64, shelf: usize, item: usize, id: &str, pixels: Pixels) {
        let set = self.board(board);
        if generation != set.generation.get() {
            return;
        }
        let Some(items) = set.posters.borrow().get(shelf).cloned() else {
            return;
        };
        put_picture(&items, item, id, pixels);
    }

    /// Downloads and shrinks a picture off the UI thread, a few at a time,
    /// then hands it to `then` on the UI thread. Failures are silent: the
    /// place for the picture just stays empty.
    fn fetch_image(
        &self,
        url: String,
        size: (u32, u32),
        then: impl FnOnce(&Rc<Library>, Pixels) + Send + 'static,
    ) {
        self.fetch_image_for(None, url, size, then)
    }

    /// As `fetch_image`, waiting while `pause` is held.
    fn fetch_image_for(
        &self,
        pause: Option<Arc<Pause>>,
        url: String,
        size: (u32, u32),
        then: impl FnOnce(&Rc<Library>, Pixels) + Send + 'static,
    ) {
        let (http, gate, cache) =
            (self.http.clone(), self.image_gate.clone(), self.picture_cache.clone());
        self.tasks.spawn(async move {
            if let Some(pause) = &pause {
                pause.pass().await;
            }
            let Ok(_turn) = gate.acquire().await else { return };
            match download_image(&http, &cache, &url, size).await {
                Ok(pixels) => on_ui(move |library| then(library, pixels)),
                Err(why) => haven_core::diag::note(&format!(
                    "picture not shown: {} ({why})",
                    haven_core::diag::plain_address(&url)
                )),
            }
        });
    }

    /// Says in Settings how much room the kept pictures take.
    fn show_cache_size(&self) {
        let cache = self.picture_cache.clone();
        self.tasks.spawn(async move {
            let (mut count, mut bytes) = (0u64, 0u64);
            if let Ok(files) = std::fs::read_dir(cache.as_path()) {
                for file in files.flatten() {
                    if let Ok(about) = file.metadata() {
                        count += 1;
                        bytes += about.len();
                    }
                }
            }
            let text = if count == 0 {
                "No pictures are kept on this computer yet.".to_owned()
            } else {
                format!("{count} pictures kept on this computer, {:.1} MB.", bytes as f64 / (1024.0 * 1024.0))
            };
            on_ui(move |library| {
                if let Some(ui) = library.ui.upgrade() {
                    ui.set_cache_status(text.into());
                }
            });
        });
    }

    /// Deletes every kept picture. Pictures on screen stay; they are
    /// fetched again the next time they are needed.
    fn clear_cache(&self) {
        self.clear_kept();
        if let Ok(files) = std::fs::read_dir(self.picture_cache.as_path()) {
            for file in files.flatten() {
                let _ = std::fs::remove_file(file.path());
            }
        }
        self.show_cache_size();
    }

    // ---- Continue Watching ----------------------------------------------

    /// Rebuilds the Continue Watching shelf from what was watched.
    fn reload_continue(&self) {
        let generation = self.continue_generation.get() + 1;
        self.continue_generation.set(generation);
        let watched = self.store.continue_watching(CONTINUE_LENGTH).unwrap_or_else(|err| {
            eprintln!("haven: could not read what was watched: {err}");
            Vec::new()
        });
        // Keep pictures that are already loaded: the shelf is rebuilt
        // every time the player closes.
        let had: Vec<Poster> = self.continuing.iter().collect();
        let mut wanted = Vec::new();
        let rows: Vec<Poster> = watched
            .iter()
            .enumerate()
            .map(|(item, progress)| {
                let old = had
                    .iter()
                    .find(|old| old.has_image && old.id == progress.title_id && old.kind == progress.kind);
                if old.is_none() {
                    if let Some(url) = &progress.poster {
                        wanted.push((item, url.clone(), progress.title_id.clone()));
                    }
                }
                let left = format!("{} left", clock(progress.duration - progress.position));
                Poster {
                    id: progress.title_id.as_str().into(),
                    kind: progress.kind.as_str().into(),
                    title: progress.title.as_str().into(),
                    year: if progress.detail.is_empty() { left } else { progress.detail.clone() }.into(),
                    image: old.map(|old| old.image.clone()).unwrap_or_default(),
                    has_image: old.is_some(),
                    video: progress.video_id.as_str().into(),
                    progress: resume::share(progress.position, progress.duration).max(0.02),
                    watched: false,
                    rating: "".into(),
                    rank: 0,
                }
            })
            .collect();
        self.continuing.set_vec(rows);
        self.fill_continue_cards(&watched);
        self.reload_history();
        for (item, url, id) in wanted {
            self.fetch_image(url, POSTER_SIZE, move |library, pixels| {
                if generation != library.continue_generation.get() {
                    return;
                }
                put_picture(&library.continuing, item, &id, pixels);
            });
        }
    }

    pub(super) fn start_history(&self, ui: &AppWindow) {
        ui.set_history(ModelRc::from(self.history.clone()));
        self.reload_history();
    }

    /// Rebuilds the Library's History: every title watched, the latest
    /// film or episode of each, newest first.
    fn reload_history(&self) {
        let generation = self.history_generation.get() + 1;
        self.history_generation.set(generation);
        let watched = self.store.history(300).unwrap_or_default();
        let had: Vec<Poster> = self.history.iter().collect();
        let mut wanted = Vec::new();
        let rows: Vec<Poster> = watched
            .iter()
            .enumerate()
            .map(|(item, progress)| {
                let old = had
                    .iter()
                    .find(|old| old.has_image && old.id == progress.title_id && old.kind == progress.kind);
                if old.is_none() {
                    if let Some(url) = &progress.poster {
                        wanted.push((item, url.clone(), progress.title_id.clone()));
                    }
                }
                let note = if !progress.detail.is_empty() {
                    progress.detail.clone()
                } else if progress.finished {
                    "Watched".to_owned()
                } else {
                    format!("{} left", clock(progress.duration - progress.position))
                };
                Poster {
                    id: progress.title_id.as_str().into(),
                    kind: progress.kind.as_str().into(),
                    title: progress.title.as_str().into(),
                    year: note.into(),
                    image: old.map(|old| old.image.clone()).unwrap_or_default(),
                    has_image: old.is_some(),
                    video: progress.video_id.as_str().into(),
                    progress: if progress.finished { 1.0 } else { resume::share(progress.position, progress.duration).max(0.02) },
                    watched: progress.finished,
                    rating: "".into(),
                    rank: 0,
                }
            })
            .collect();
        self.history.set_vec(rows);
        for (item, url, id) in wanted {
            self.fetch_image(url, POSTER_SIZE, move |library, pixels| {
                if generation != library.history_generation.get() {
                    return;
                }
                put_picture(&library.history, item, &id, pixels);
            });
        }
    }

    /// Takes a title off Continue Watching: forgets how far it (and, for
    /// a series, each of its episodes) was watched.
    fn remove_continue(&self, poster: &Poster) {
        if let Err(err) = self.store.remove_progress(poster.kind.as_str(), poster.id.as_str()) {
            return haven_core::diag::note(&format!("could not clear what was watched: {err}"));
        }
        self.reload_continue();
        self.show_resume_note();
    }

    // ---- Watchlist --------------------------------------------------------

    /// Rebuilds the Watchlist page from the database.
    fn reload_watchlist(&self) {
        let generation = self.watchlist_generation.get() + 1;
        self.watchlist_generation.set(generation);
        let saved = self.store.watchlist().unwrap_or_else(|err| {
            eprintln!("haven: could not read the watchlist: {err}");
            Vec::new()
        });
        // Keep pictures that are already loaded.
        let had: Vec<Poster> = self.watchlist.iter().collect();
        let mut wanted = Vec::new();
        // Titles saved without a poster (brought in from Trakt or Simkl,
        // say): looked up so their pictures show here too.
        let mut missing = Vec::new();
        let rows: Vec<Poster> = saved
            .iter()
            .enumerate()
            .map(|(item, saved)| {
                let old = had
                    .iter()
                    .find(|old| old.has_image && old.id == saved.title_id && old.kind == saved.kind);
                if old.is_none() {
                    let poster = saved.poster.clone().or_else(|| {
                        let kept = self.kept_details(&saved.kind, &saved.title_id)?.poster?;
                        let _ = self.store.add_to_watchlist(&Saved { poster: Some(kept.clone()), ..saved.clone() });
                        Some(kept)
                    });
                    match poster {
                        Some(url) => wanted.push((item, url, saved.title_id.clone())),
                        None => missing.push((item, saved.clone())),
                    }
                }
                Poster {
                    id: saved.title_id.as_str().into(),
                    kind: saved.kind.as_str().into(),
                    title: saved.title.as_str().into(),
                    year: saved.year.as_str().into(),
                    image: old.map(|old| old.image.clone()).unwrap_or_default(),
                    has_image: old.is_some(),
                    video: "".into(),
                    progress: 0.0,
                    watched: self.film_watched(&saved.kind, &saved.title_id),
                    rating: "".into(),
                    rank: 0,
                }
            })
            .collect();
        self.watchlist.set_vec(rows);
        self.refresh_calendar();
        for (item, saved) in missing {
            let id = saved.title_id.clone();
            let details = self.details_of(&saved.kind, &saved.title_id);
            // An IMDb title nobody has details (or a poster) for yet, such as
            // a brand-new series: the picture service Cinemeta itself uses.
            let fallback = haven_metadata::is_imdb_id(&saved.title_id)
                .then(|| format!("https://images.metahub.space/poster/medium/{}/img", saved.title_id));
            if details.is_none() && fallback.is_none() {
                continue;
            }
            self.tasks.spawn(async move {
                let found = match details {
                    Some(details) => details.await.ok().and_then(|meta| meta.poster),
                    None => None,
                };
                let Some(url) = found.or(fallback) else { return };
                on_ui(move |library| {
                    let _ = library.store.add_to_watchlist(&Saved { poster: Some(url.clone()), ..saved });
                    if generation != library.watchlist_generation.get() {
                        return;
                    }
                    library.fetch_image(url, POSTER_SIZE, move |library, pixels| {
                        if generation != library.watchlist_generation.get() {
                            return;
                        }
                        put_picture(&library.watchlist, item, &id, pixels);
                    });
                });
            });
        }
        for (item, url, id) in wanted {
            self.fetch_image(url, POSTER_SIZE, move |library, pixels| {
                if generation != library.watchlist_generation.get() {
                    return;
                }
                put_picture(&library.watchlist, item, &id, pixels);
            });
        }
    }

    /// Puts the open title on the watchlist, or takes it off.
    fn toggle_watchlist(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Some((kind, id, year, poster)) = self.open.borrow().as_ref().map(|open| {
            let meta = open.meta.as_ref();
            (
                open.kind.clone(),
                open.id.clone(),
                meta.and_then(|meta| meta.release_info.clone()).unwrap_or_default(),
                meta.and_then(|meta| meta.poster.clone()),
            )
        }) else {
            return;
        };
        let detail = ui.get_detail();
        let changed = if detail.saved {
            self.store.remove_from_watchlist(&kind, &id)
        } else {
            self.store.add_to_watchlist(&Saved {
                kind,
                title_id: id,
                title: detail.title.to_string(),
                year,
                poster,
                added_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_millis() as i64),
            })
        };
        match changed {
            Ok(()) => {
                self.update_detail(|detail| detail.saved = !detail.saved);
                self.reload_watchlist();
                if let Some((kind, id)) =
                    self.open.borrow().as_ref().map(|open| (open.kind.clone(), open.id.clone()))
                {
                    self.track_watchlist(&kind, &id, !detail.saved);
                }
            }
            Err(err) => eprintln!("haven: could not change the watchlist: {err}"),
        }
    }

    /// A film's "Sources": looks up, now, where it can be watched.
    fn show_film_sources(&self) {
        let Some((kind, id)) = self.open.borrow().as_ref().map(|open| (open.kind.clone(), open.id.clone())) else {
            return;
        };
        self.load_streams(&kind, &id, sources::HEADING);
    }

    // ---- Pulled down to refresh ----------------------------------------

    /// Loads Home afresh: every shelf, the servers' libraries, the
    /// calendar's dates and Continue Watching. What is on screen stays
    /// until each fresh answer replaces it.
    fn refresh_home(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else { return };
        haven_core::diag::note("home: loading afresh (pulled down)");
        ui.set_home_refreshing(true);
        self.reload_home();
        self.reload_catalog();
        self.reload_server_shelves();
        self.recheck_calendar();
        self.recheck_continue();
        self.reload_continue();
        self.wait_for_refresh(std::time::Instant::now());
    }

    /// Turns the sign until nothing asked for is still out (at most 20 s).
    fn wait_for_refresh(self: &Rc<Self>, started: std::time::Instant) {
        if self.fetches_left.get() == 0 || started.elapsed().as_secs() >= 20 {
            if let Some(ui) = self.ui.upgrade() {
                ui.set_home_refreshing(false);
            }
            return;
        }
        let library = Rc::downgrade(self);
        slint::Timer::single_shot(std::time::Duration::from_millis(250), move || {
            if let Some(library) = library.upgrade() {
                library.wait_for_refresh(started);
            }
        });
    }

    // ---- A title's page -------------------------------------------------

    /// A film watched to the end (a series has no single tick).
    pub(super) fn film_watched(&self, kind: &str, id: &str) -> bool {
        kind == "movie" && self.store.finished_videos(kind, id).is_ok_and(|done| !done.is_empty())
    }

    /// The episodes of the open title watched to the end.
    fn finished_here(&self) -> std::collections::HashSet<String> {
        let open = self.open.borrow();
        let Some(open) = open.as_ref() else { return Default::default() };
        self.store.finished_videos(&open.kind, &open.id).unwrap_or_default().into_iter().collect()
    }

    fn update_detail(&self, change: impl FnOnce(&mut Detail)) {
        if let Some(ui) = self.ui.upgrade() {
            let mut detail = ui.get_detail();
            change(&mut detail);
            ui.set_detail(detail);
        }
    }

    fn set_streams_state(&self, title: Option<&str>, loading: bool, note: &str) {
        if let Some(ui) = self.ui.upgrade() {
            if let Some(title) = title {
                ui.set_streams_title(title.into());
            }
            ui.set_streams_loading(loading);
            ui.set_streams_note(note.into());
        }
    }

    /// Opens a title's page from its poster and asks the add-ons for its
    /// details.
    fn open_title(&self, poster: Poster) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = self.detail_generation.get() + 1;
        self.detail_generation.set(generation);
        self.stream_generation.set(self.stream_generation.get() + 1);

        let (kind, id) = (poster.kind.to_string(), poster.id.to_string());
        self.seasons.set_vec(Vec::new());
        self.episodes.set_vec(Vec::new());
        self.clear_streams();
        self.clear_offers("");
        self.set_streams_state(Some(sources::HEADING), false, "");
        // A Continue Watching card names the episode to go back to; its
        // second line is that episode, not the title's year.
        let wanted = (!poster.video.is_empty()).then(|| poster.video.to_string());
        ui.set_detail(Detail {
            title: poster.title,
            facts: if wanted.is_some() { "".into() } else { poster.year },
            has_poster: poster.has_image,
            poster: poster.image,
            loading: true,
            saved: self.store.in_watchlist(&kind, &id).unwrap_or(false),
            watched: self.film_watched(&kind, &id),
            film: kind == "movie",
            ..Detail::default()
        });
        ui.set_episode_open(false);
        // The page opens on the poster and details alone.
        ui.set_detail_panel_open(false);
        ui.set_play_note("".into());
        ui.set_episode_label("".into());
        ui.set_episode_filter("".into());
        ui.set_detail_open(true);

        let sources: Vec<String> = self
            .installed
            .borrow()
            .iter()
            .filter(|addon| addon.provides("meta", &kind, &id))
            .map(|addon| addon.url.clone())
            .collect();
        *self.open.borrow_mut() =
            Some(OpenTitle { kind: kind.clone(), id: id.clone(), meta: None, wanted });
        // Shown as it was last time straight away; the fresh details
        // below only change the page if something is new.
        if let Some(kept) = self.kept_details(&kind, &id) {
            self.detail_loaded(generation, Ok(kept));
        }
        if let Some((server, item)) = self.server_item(&id) {
            let client = self.server_client.clone();
            self.tasks.spawn(async move {
                let found = client
                    .meta(&server, &item)
                    .await
                    .map_err(|err| format!("Could not load the details: {err}."));
                on_ui(move |library| library.detail_loaded(generation, found));
            });
            return;
        }
        if let Some((series, number)) = haven_metadata::split_id(&id) {
            let Some(tmdb) = self.tmdb() else {
                return self.detail_loaded(
                    generation,
                    Err("Add a TMDB key in Settings to see this title's details.".into()),
                );
            };
            self.tasks.spawn(async move {
                let found = tmdb
                    .title(series, number)
                    .await
                    .map_err(|err| format!("Could not load the details: {err}."));
                on_ui(move |library| library.detail_loaded(generation, found));
            });
            return;
        }
        if sources.is_empty() {
            return self.detail_loaded(
                generation,
                Err(NO_DETAILS.into()),
            );
        }
        let addons = self.catalogues.clone();
        self.tasks.spawn(async move {
            // The first source that knows the title wins.
            let mut found = Err(NO_DETAILS.to_owned());
            for url in sources {
                match addons.meta(&url, &kind, &id).await {
                    Ok(meta) => {
                        found = Ok(meta);
                        break;
                    }
                    Err(err) => found = Err(format!("Could not load the details: {err}.")),
                }
            }
            on_ui(move |library| library.detail_loaded(generation, found));
        });
    }

    fn detail_loaded(&self, generation: u64, found: Result<MetaDetail, String>) {
        if generation != self.detail_generation.get() {
            return;
        }
        let Some((kind, id)) =
            self.open.borrow().as_ref().map(|open| (open.kind.clone(), open.id.clone()))
        else {
            return;
        };
        // What the page shows already (kept from last time), if anything.
        let shown = self.open.borrow().as_ref().and_then(|open| open.meta.clone());
        let meta = match found {
            Ok(meta) if shown.as_ref() == Some(&meta) => {
                // Nothing new: the page stays exactly as it is.
                return self.update_detail(|detail| detail.loading = false);
            }
            Ok(meta) => meta,
            // The kept details stay when fresh ones cannot be had.
            Err(_) if shown.is_some() => return self.update_detail(|detail| detail.loading = false),
            Err(err) => {
                self.update_detail(|detail| {
                    detail.loading = false;
                    detail.error = err.into();
                });
                return;
            }
        };

        let mut facts: Vec<String> = Vec::new();
        facts.extend(meta.release_info.clone());
        facts.extend(meta.runtime.clone());
        facts.extend(meta.rating.as_ref().map(|rating| format!("★ {rating}")));
        if !meta.genres.is_empty() {
            facts.push(meta.genres.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
        }
        self.keep(kept::details_name(&kind, &id), meta.clone());
        let needs_poster = self.ui.upgrade().is_some_and(|ui| !ui.get_detail().has_poster);
        self.update_detail(|detail| {
            detail.loading = false;
            detail.title = meta.name.as_str().into();
            detail.facts = facts.join("  ·  ").into();
            detail.description = meta.description.as_deref().unwrap_or_default().trim().into();
            detail.cast =
                meta.cast.iter().take(CAST_SHOWN).cloned().collect::<Vec<_>>().join(", ").into();
            detail.director = meta.director.join(", ").into();
        });
        let refreshing = shown.is_some();
        let same_background = shown.as_ref().is_some_and(|old| old.background == meta.background);
        let pictures = [
            (meta.poster.clone().filter(|_| needs_poster), POSTER_SIZE, DetailImage::Poster),
            (meta.background.clone().filter(|_| !same_background), BACKGROUND_SIZE, DetailImage::Background),
        ];
        for (url, size, which) in pictures {
            if let Some(url) = url {
                self.fetch_image(url, size, move |library, pixels| {
                    library.detail_image_loaded(generation, which, pixels);
                });
            }
        }

        let seasons = meta.seasons();
        // Fresh details for a page already showing: new episodes and
        // text come in, the season and episode picked stay as they are.
        if refreshing {
            let season = self.ui.upgrade().map(|ui| ui.get_season());
            if let Some(open) = self.open.borrow_mut().as_mut() {
                open.meta = Some(meta);
            }
            if !seasons.is_empty() {
                self.seasons.set_vec(seasons.iter().map(|&s| s as i32).collect::<Vec<_>>());
                let keep = season.filter(|s| seasons.contains(&(*s as u32))).unwrap_or(seasons[0] as i32);
                self.show_season(keep);
            }
            return;
        }

        self.enrich(generation, &meta);
        self.load_offers(generation, &meta);

        let has_videos = !meta.videos.is_empty();
        let finished_now = self.finished_here();
        let all: Vec<EpisodeRow> = meta
            .videos
            .iter()
            .map(|video| EpisodeRow { watched: finished_now.contains(video.id.as_str()), ..episode_row(video) })
            .collect();
        // The episode a Continue Watching card asked for, if it exists.
        let wanted = self.open.borrow_mut().as_mut().and_then(|open| open.wanted.take());
        let wanted = wanted.and_then(|id| meta.videos.iter().find(|video| video.id == id).cloned());
        if let Some(open) = self.open.borrow_mut().as_mut() {
            open.meta = Some(meta);
        }
        // A film's places to watch are looked up when "Sources" is tapped.
        if !has_videos {
            self.set_streams_state(Some(sources::HEADING), false, "");
        } else {
            self.set_streams_state(
                Some(sources::HEADING),
                false,
                "Choose an episode to see where you can watch it.",
            );
            match seasons.first() {
                Some(&first) => {
                    self.seasons.set_vec(seasons.iter().map(|&s| s as i32).collect::<Vec<_>>());
                    let season = wanted.as_ref().and_then(|video| video.season).unwrap_or(first);
                    self.show_season(season as i32);
                }
                // Videos with no seasons (a channel's uploads): list them all.
                None => self.episodes.set_vec(all),
            }
            if let Some(video) = wanted {
                self.open_episode(&video.id);
            }
        }
    }

    fn detail_image_loaded(&self, generation: u64, which: DetailImage, pixels: Pixels) {
        if generation != self.detail_generation.get() {
            return;
        }
        self.update_detail(|detail| match which {
            DetailImage::Poster => {
                detail.poster = Image::from_rgba8(pixels);
                detail.has_poster = true;
            }
            DetailImage::Background => {
                detail.background = Image::from_rgba8(pixels);
                detail.has_background = true;
            }
        });
    }

    fn show_season(&self, season: i32) {
        // Narrowed to the words typed over the list, if any.
        let words = self.ui.upgrade().map(|ui| ui.get_episode_filter().to_lowercase()).unwrap_or_default();
        let words = words.trim();
        let picked = self.listed.borrow().as_ref().map(|listed| listed.video_id.clone());
        let finished = self.finished_here();
        let rows: Vec<EpisodeRow> = match self.open.borrow().as_ref().and_then(|o| o.meta.as_ref()) {
            Some(meta) => meta
                .episodes(season.max(0) as u32)
                .into_iter()
                .map(|video| {
                    let mut row = episode_row(video);
                    row.selected = picked.as_deref() == Some(video.id.as_str());
                    row.watched = finished.contains(video.id.as_str());
                    row
                })
                .filter(|row| {
                    words.is_empty()
                        || row.title.to_lowercase().contains(words)
                        || row.number.to_lowercase().contains(words)
                })
                .collect(),
            None => return,
        };
        self.episodes.set_vec(rows);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_season(season);
        }
        self.update_season_flag();
    }

    /// The words over the episode list changed: show the season again.
    fn filter_episodes(&self, _words: &str) {
        if let Some(season) = self.ui.upgrade().map(|ui| ui.get_season()) {
            self.show_season(season);
        }
    }

    fn open_episode(&self, video_id: &str) {
        let Some(kind) = self.open.borrow().as_ref().map(|open| open.kind.clone()) else {
            return;
        };
        let mut heading = sources::HEADING.to_owned();
        let mut detail = String::new();
        let season = self.ui.upgrade().map(|ui| ui.get_season()).filter(|_| self.seasons.row_count() > 0);
        for row in 0..self.episodes.row_count() {
            let Some(mut episode) = self.episodes.row_data(row) else { continue };
            let selected = episode.id == video_id;
            if selected {
                heading = format!("{} {} {}", sources::HEADING, episode.number, episode.title);
                let number = match season {
                    Some(season) if !episode.number.is_empty() => format!("S{season} {}", episode.number),
                    _ => episode.number.to_string(),
                };
                detail = [number.as_str(), episode.title.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
            }
            if episode.selected != selected {
                episode.selected = selected;
                self.episodes.set_row_data(row, episode);
            }
        }
        self.load_streams(&kind, video_id, heading.trim());
        if let Some(ui) = self.ui.upgrade() {
            ui.set_episode_label(detail.as_str().into());
            ui.set_episode_open(true);
            ui.set_detail_panel_open(true);
        }
        if let Some(listed) = self.listed.borrow_mut().as_mut() {
            listed.detail = detail;
        }
    }

    /// Says where the listed film or episode would resume, if anywhere.
    fn show_resume_note(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let saved = match (self.open.borrow().as_ref(), self.listed.borrow().as_ref()) {
            (Some(open), Some(listed)) if self.player.borrow().is_some() => self
                .store
                .progress(&open.kind, &open.id, &listed.video_id)
                .unwrap_or_default(),
            _ => None,
        };
        let note = match saved {
            Some(saved) if saved.finished => "You have watched this.".to_owned(),
            Some(saved) => match resume::resume_from(saved.position, saved.duration, false) {
                Some(at) => format!(
                    "Resumes at {} of {}.",
                    clock(at),
                    clock(saved.duration)
                ),
                None => String::new(),
            },
            None => String::new(),
        };
        ui.set_resume_note(note.into());
    }

    fn close_detail(&self) {
        self.detail_generation.set(self.detail_generation.get() + 1);
        self.stream_generation.set(self.stream_generation.get() + 1);
        *self.open.borrow_mut() = None;
        *self.listed.borrow_mut() = None;
        if let Some(ui) = self.ui.upgrade() {
            ui.set_detail_open(false);
        }
    }
}

fn episode_row(video: &Video) -> EpisodeRow {
    let title = match (&video.title, video.episode) {
        (Some(title), _) => title.clone(),
        (None, Some(number)) => format!("Episode {number}"),
        (None, None) => "Untitled".to_owned(),
    };
    EpisodeRow {
        id: video.id.as_str().into(),
        number: video.episode.map(|number| format!("E{number}")).unwrap_or_default().into(),
        title: title.into(),
        // "2019-01-27T05:00:00.000Z" -> "2019-01-27"
        date: video.released.as_deref().and_then(|date| date.get(..10)).unwrap_or_default().into(),
        selected: false,
        watched: false,
    }
}

/// The row of `items` a picture fetched for row `item` (title `id`)
/// belongs on now: that row if it still holds the title, else the first
/// row with the title that has no picture yet (the list may have changed
/// while the picture came), else none.
fn row_for(items: &VecModel<Poster>, item: usize, id: &str) -> Option<usize> {
    if items.row_data(item).is_some_and(|p| p.id.as_str() == id) {
        return Some(item);
    }
    items.iter().position(|p| p.id.as_str() == id && !p.has_image)
}

/// Puts a fetched picture on the card of title `id`.
fn put_picture(items: &VecModel<Poster>, item: usize, id: &str, pixels: Pixels) {
    let Some(at) = row_for(items, item, id) else { return };
    if let Some(mut poster) = items.row_data(at) {
        poster.image = Image::from_rgba8(pixels);
        poster.has_image = true;
        items.set_row_data(at, poster);
    }
}

/// The file a picture is kept in: named after its address (without what
/// follows `?`, so a new sign-in token does not make it a new picture)
/// and the size asked for, which some addresses only carry after `?`.
fn cached_picture(cache: &std::path::Path, url: &str, size: (u32, u32)) -> std::path::PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    haven_core::diag::plain_address(url).hash(&mut hasher);
    size.hash(&mut hasher);
    cache.join(format!("{:016x}", hasher.finish()))
}

/// Shrinks a downloaded picture to fit `size`.
fn read_picture(bytes: Vec<u8>, size: (u32, u32)) -> Result<Pixels, String> {
    let length = bytes.len();
    let start: Vec<String> = bytes.iter().take(8).map(|byte| format!("{byte:02x}")).collect();
    let unreadable = |why: String| format!("{why}; {length} bytes starting {}", start.join(" "));
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| unreadable(err.to_string()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8000);
    limits.max_image_height = Some(8000);
    reader.limits(limits);
    let small =
        reader.decode().map_err(|err| unreadable(err.to_string()))?.thumbnail(size.0, size.1).into_rgba8();
    Ok(SharedPixelBuffer::clone_from_slice(small.as_raw(), small.width(), small.height()))
}

/// Gets a picture, from the pictures kept on this computer if it is
/// there, else from the network (and then keeps it). On failure, says
/// why in a few words (for the log).
async fn download_image(
    http: &reqwest::Client,
    cache: &std::path::Path,
    url: &str,
    size: (u32, u32),
) -> Result<Pixels, String> {
    let kept = cached_picture(cache, url, size);
    let from_disk = {
        let kept = kept.clone();
        tokio::task::spawn_blocking(move || {
            let bytes = std::fs::read(&kept).ok()?;
            let pixels = read_picture(bytes, size).ok();
            if pixels.is_none() {
                // A damaged file: fetch the picture again.
                let _ = std::fs::remove_file(&kept);
            }
            pixels
        })
        .await
        .ok()
        .flatten()
    };
    if let Some(pixels) = from_disk {
        return Ok(pixels);
    }
    let bytes = http::get_bytes(http, url, IMAGE_BYTES).await.map_err(|err| match err {
        // The full error would repeat the address, token and all.
        http::HttpError::Request(err) if err.is_timeout() => "the server took too long".to_owned(),
        http::HttpError::Request(_) => "the server could not be reached".to_owned(),
        other => other.to_string(),
    })?;
    tokio::task::spawn_blocking(move || {
        let pixels = read_picture(bytes.clone(), size)?;
        // Kept only once it is known to be a picture. Written under
        // another name first so a half-written file is never read.
        if let Some(folder) = kept.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let part = kept.with_extension("part");
        if std::fs::write(&part, &bytes).is_ok() {
            let _ = std::fs::rename(&part, &kept);
        }
        Ok(pixels)
    })
    .await
    .map_err(|_| "the picture could not be read".to_owned())?
}

/// The id this copy of Haven gives media servers, made once and kept.
fn device_id(store: &Store) -> String {
    const KEY: &str = "device_id";
    if let Ok(Some(id)) = store.get::<String>(KEY) {
        if !id.is_empty() {
            return id;
        }
    }
    // Two fresh hasher seeds: random enough to tell copies of Haven apart.
    use std::hash::{BuildHasher, Hasher};
    let part = || std::collections::hash_map::RandomState::new().build_hasher().finish();
    let id = format!("haven-{:016x}{:016x}", part(), part());
    if let Err(err) = store.put(KEY, &id) {
        eprintln!("haven: could not save the device id: {err}");
    }
    id
}

#[cfg(test)]
mod tests {
    use slint::{Model, VecModel};

    use super::{Poster, row_for};

    fn card(id: &str, has_image: bool) -> Poster {
        Poster { id: id.into(), has_image, ..Poster::default() }
    }

    #[test]
    fn a_late_picture_goes_to_its_own_title() {
        // Asked for when row 0 held The Hunger Games; the fresh list put
        // The Odyssey there and moved The Hunger Games to row 1.
        let items = VecModel::from(vec![card("odyssey", false), card("hunger-games", false)]);
        assert_eq!(row_for(&items, 0, "hunger-games"), Some(1));
        assert_eq!(row_for(&items, 0, "odyssey"), Some(0));
        // A title no longer listed gets no picture.
        assert_eq!(row_for(&items, 0, "gone"), None);
        assert_eq!(items.row_count(), 2);
    }
}

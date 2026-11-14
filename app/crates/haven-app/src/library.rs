//! Add-ons and everything they fill: the Home shelves, search results, a
//! title's page with its episodes, and its streams. Network work runs off
//! the UI thread and comes back through [`on_ui`].

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use haven_addons::{
    Catalog, Client, Manifest, MetaDetail, MetaPreview, Stream, StreamKind, Video, manifest_url,
    parse_manifest,
};
use haven_core::{
    http,
    storage::{Saved, Store},
};
use haven_player::{clock, resume, source::web_address};
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use tokio::{runtime::Handle, sync::Semaphore};

use crate::{AddonRow, AppWindow, Detail, EpisodeRow, Poster, Shelf, StreamRow};

/// Titles shown per shelf (a catalogue's first page can be far longer).
const SHELF_LENGTH: usize = 30;
const IMAGE_BYTES: usize = 6 * 1024 * 1024;
/// Posters are kept at twice the card size, for sharp 2x screens.
const POSTER_SIZE: (u32, u32) = (300, 450);
const BACKGROUND_SIZE: (u32, u32) = (1600, 900);
const IMAGES_AT_ONCE: usize = 6;
const CAST_SHOWN: usize = 6;
const CONTINUE_LENGTH: u32 = 20;

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

struct Installed {
    url: String,
    manifest: Manifest,
}

/// The two places that show shelves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Board {
    Home,
    Search,
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
}

/// A shelf to build: which catalogue of which add-on, and its heading.
struct ShelfPlan {
    title: String,
    source: String,
    url: String,
    catalog: Catalog,
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
}

#[derive(Clone, Copy)]
enum DetailImage {
    Poster,
    Background,
}

pub struct Library {
    ui: slint::Weak<AppWindow>,
    store: Rc<Store>,
    tasks: Handle,
    addons: Client,
    http: reqwest::Client,
    installed: RefCell<Vec<Installed>>,
    home: ShelfSet,
    search: ShelfSet,
    image_gate: Arc<Semaphore>,

    open: RefCell<Option<OpenTitle>>,
    /// Goes up when another title is opened or the page is closed.
    detail_generation: Cell<u64>,
    seasons: Rc<VecModel<i32>>,
    episodes: Rc<VecModel<EpisodeRow>>,
    streams: Rc<VecModel<StreamRow>>,
    /// Goes up whenever streams are asked for again.
    stream_generation: Cell<u64>,
    /// Add-ons that have not answered the current stream request yet.
    streams_pending: Cell<usize>,
    /// The address behind each row of `streams`; `None` if not playable.
    stream_addresses: RefCell<Vec<Option<String>>>,
    listed: RefCell<Option<Listed>>,
    /// Plays a stream; unset in a build without the player.
    player: RefCell<Option<Box<dyn Fn(PlayRequest)>>>,
    continuing: Rc<VecModel<Poster>>,
    continue_generation: Cell<u64>,
    watchlist: Rc<VecModel<Poster>>,
    watchlist_generation: Cell<u64>,
}

impl Library {
    /// Loads the installed add-ons, hooks up the UI and starts filling Home.
    pub fn start(
        ui: &AppWindow,
        store: Rc<Store>,
        tasks: Handle,
    ) -> Result<Rc<Self>, http::HttpError> {
        let http = http::client()?;
        let library = Rc::new(Self {
            ui: ui.as_weak(),
            store,
            tasks,
            addons: Client::new(http.clone()),
            http,
            installed: RefCell::default(),
            home: ShelfSet::default(),
            search: ShelfSet::default(),
            image_gate: Arc::new(Semaphore::new(IMAGES_AT_ONCE)),
            open: RefCell::default(),
            detail_generation: Cell::new(0),
            seasons: Rc::default(),
            episodes: Rc::default(),
            streams: Rc::default(),
            stream_generation: Cell::new(0),
            streams_pending: Cell::new(0),
            stream_addresses: RefCell::default(),
            listed: RefCell::default(),
            player: RefCell::default(),
            continuing: Rc::default(),
            continue_generation: Cell::new(0),
            watchlist: Rc::default(),
            watchlist_generation: Cell::new(0),
        });
        CURRENT.with(|current| *current.borrow_mut() = Some(library.clone()));

        ui.set_shelves(ModelRc::from(library.home.shelves.clone()));
        ui.set_search_shelves(ModelRc::from(library.search.shelves.clone()));
        ui.set_seasons(ModelRc::from(library.seasons.clone()));
        ui.set_episodes(ModelRc::from(library.episodes.clone()));
        ui.set_streams(ModelRc::from(library.streams.clone()));
        ui.set_watchlist(ModelRc::from(library.watchlist.clone()));
        ui.set_continue_shelf(Shelf {
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
        connect!(on_install_addon, |address| |l: &Rc<Self>| l.install(address.as_str()));
        connect!(on_remove_addon, |url| |l: &Rc<Self>| l.remove(url.as_str()));
        connect!(on_search, |query| |l: &Rc<Self>| l.run_search(query.as_str()));
        connect!(on_open_title, |poster| |l: &Rc<Self>| l.open_title(poster));
        connect!(on_close_detail, | | |l: &Rc<Self>| l.close_detail());
        connect!(on_season_selected, |season| |l: &Rc<Self>| l.show_season(season));
        connect!(on_episode_selected, |id| |l: &Rc<Self>| l.open_episode(id.as_str()));
        connect!(on_play_stream, |row| |l: &Rc<Self>| l.play_stream(row));
        connect!(on_toggle_watchlist, | | |l: &Rc<Self>| l.toggle_watchlist());

        library.load_installed();
        library.reload_home();
        library.reload_continue();
        library.reload_watchlist();
        Ok(library)
    }

    /// Gives the library a player; from then on Direct streams can be
    /// played. Without one they are only listed.
    #[cfg_attr(not(feature = "player"), allow(dead_code))]
    pub fn set_player(&self, play: impl Fn(PlayRequest) + 'static) {
        *self.player.borrow_mut() = Some(Box::new(play));
    }

    /// Call when the player has closed: what was watched may have changed.
    #[cfg_attr(not(feature = "player"), allow(dead_code))]
    pub fn player_closed(&self) {
        self.reload_continue();
        self.show_resume_note();
    }

    fn board(&self, board: Board) -> &ShelfSet {
        match board {
            Board::Home => &self.home,
            Board::Search => &self.search,
        }
    }

    // ---- Add-ons ----------------------------------------------------

    /// Reads the installed add-ons from the database into memory and the UI.
    fn load_installed(&self) {
        let stored = self.store.addons().unwrap_or_else(|err| {
            eprintln!("haven: could not read the installed add-ons: {err}");
            Vec::new()
        });
        let installed: Vec<Installed> = stored
            .into_iter()
            .filter_map(|addon| match parse_manifest(&addon.manifest) {
                Ok(manifest) => Some(Installed { url: addon.url, manifest }),
                Err(err) => {
                    eprintln!("haven: skipping add-on {}: {err}", addon.url);
                    None
                }
            })
            .collect();

        let rows: Vec<AddonRow> = installed
            .iter()
            .map(|addon| AddonRow {
                url: addon.url.as_str().into(),
                name: addon.manifest.name.as_str().into(),
                description: addon.manifest.description.as_deref().unwrap_or_default().into(),
                version: addon.manifest.version.as_str().into(),
                catalogs: addon.manifest.catalogs.iter().filter(|c| !c.needs_input()).count() as i32,
            })
            .collect();
        *self.installed.borrow_mut() = installed;
        if let Some(ui) = self.ui.upgrade() {
            ui.set_addons(ModelRc::new(VecModel::from(rows)));
        }
    }

    fn set_status(&self, text: &str, is_error: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_addon_status(text.into());
            ui.set_addon_status_is_error(is_error);
        }
    }

    fn set_busy(&self, busy: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_addon_busy(busy);
        }
    }

    /// Fetches the manifest at the address the user gave and installs it.
    fn install(&self, address: &str) {
        let url = match manifest_url(address) {
            Ok(url) => url,
            Err(err) => {
                return self.set_status(&format!("Could not install the add-on: {err}."), true);
            }
        };
        self.set_busy(true);
        self.set_status("Contacting the add-on…", false);
        let addons = self.addons.clone();
        self.tasks.spawn(async move {
            let fetched = async {
                let json = addons.manifest_json(&url).await?;
                let manifest = parse_manifest(&json)?;
                Ok::<_, haven_addons::Error>((json, manifest.name))
            }
            .await
            .map_err(|err| err.to_string());
            on_ui(move |library| library.install_finished(&url, fetched));
        });
    }

    fn install_finished(&self, url: &str, fetched: Result<(String, String), String>) {
        self.set_busy(false);
        let (json, name) = match fetched {
            Ok(fetched) => fetched,
            Err(err) => {
                return self.set_status(&format!("Could not install the add-on: {err}."), true);
            }
        };
        if let Err(err) = self.store.save_addon(url, &json) {
            return self.set_status(&format!("Could not save the add-on: {err}"), true);
        }
        self.load_installed();
        self.reload_home();
        self.clear_search();
        self.set_status(&format!("Installed {name}."), false);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_addon_address("".into());
        }
    }

    fn remove(&self, url: &str) {
        if let Err(err) = self.store.remove_addon(url) {
            return self.set_status(&format!("Could not remove the add-on: {err}"), true);
        }
        self.load_installed();
        self.reload_home();
        self.clear_search();
        self.set_status("Add-on removed.", false);
    }

    // ---- Shelves (Home and Search) ------------------------------------

    /// Rebuilds Home: one shelf per catalogue that needs no input.
    fn reload_home(&self) {
        let plans = self
            .installed
            .borrow()
            .iter()
            .flat_map(|addon| {
                addon.manifest.catalogs.iter().filter(|c| !c.needs_input()).map(|catalog| {
                    ShelfPlan {
                        title: catalog.title(),
                        source: addon.manifest.name.clone(),
                        url: addon.url.clone(),
                        catalog: catalog.clone(),
                    }
                })
            })
            .collect();
        self.fill(Board::Home, plans, None);
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
                addon.manifest.catalogs.iter().filter(|c| c.searchable()).map(|catalog| {
                    ShelfPlan {
                        title: catalog.results_title(),
                        source: addon.manifest.name.clone(),
                        url: addon.url.clone(),
                        catalog: catalog.clone(),
                    }
                })
            })
            .collect();
        let note = if self.installed.borrow().is_empty() {
            "Install an add-on first: search looks through your add-ons."
        } else if plans.is_empty() {
            "None of your add-ons can be searched."
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

        let mut shelves = Vec::new();
        let mut posters = Vec::new();
        for (shelf, plan) in plans.into_iter().enumerate() {
            let items = Rc::new(VecModel::<Poster>::default());
            shelves.push(Shelf {
                title: plan.title.into(),
                source: plan.source.into(),
                loading: true,
                error: "".into(),
                items: ModelRc::from(items.clone()),
            });
            posters.push(items);

            let (addons, search) = (self.addons.clone(), search.clone());
            self.tasks.spawn(async move {
                let metas = addons
                    .catalog(&plan.url, &plan.catalog, search.as_deref())
                    .await
                    .map_err(|err| err.to_string());
                on_ui(move |library| library.shelf_loaded(board, generation, shelf, metas));
            });
        }
        *set.posters.borrow_mut() = posters;
        set.shelves.set_vec(shelves);
    }

    fn shelf_loaded(
        &self,
        board: Board,
        generation: u64,
        shelf: usize,
        metas: Result<Vec<MetaPreview>, String>,
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
        row.loading = false;
        match metas {
            Err(err) => row.error = format!("Could not load this: {err}.").into(),
            Ok(metas) => {
                let metas: Vec<MetaPreview> = metas.into_iter().take(SHELF_LENGTH).collect();
                items.set_vec(
                    metas
                        .iter()
                        .map(|meta| Poster {
                            id: meta.id.as_str().into(),
                            kind: meta.kind.as_str().into(),
                            title: meta.name.as_str().into(),
                            year: meta.release_info.as_deref().unwrap_or_default().into(),
                            image: Image::default(),
                            has_image: false,
                            video: "".into(),
                            progress: 0.0,
                        })
                        .collect::<Vec<_>>(),
                );
                for (item, meta) in metas.into_iter().enumerate() {
                    let Some(url) = meta.poster else { continue };
                    self.fetch_image(url, POSTER_SIZE, move |library, pixels| {
                        library.poster_loaded(board, generation, shelf, item, pixels);
                    });
                }
            }
        }
        set.shelves.set_row_data(shelf, row);
    }

    fn poster_loaded(&self, board: Board, generation: u64, shelf: usize, item: usize, pixels: Pixels) {
        let set = self.board(board);
        if generation != set.generation.get() {
            return;
        }
        let Some(items) = set.posters.borrow().get(shelf).cloned() else {
            return;
        };
        if let Some(mut poster) = items.row_data(item) {
            poster.image = Image::from_rgba8(pixels);
            poster.has_image = true;
            items.set_row_data(item, poster);
        }
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
        let (http, gate) = (self.http.clone(), self.image_gate.clone());
        self.tasks.spawn(async move {
            let Ok(_turn) = gate.acquire().await else { return };
            if let Some(pixels) = download_image(&http, &url, size).await {
                on_ui(move |library| then(library, pixels));
            }
        });
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
                        wanted.push((item, url.clone()));
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
                }
            })
            .collect();
        self.continuing.set_vec(rows);
        for (item, url) in wanted {
            self.fetch_image(url, POSTER_SIZE, move |library, pixels| {
                if generation != library.continue_generation.get() {
                    return;
                }
                if let Some(mut poster) = library.continuing.row_data(item) {
                    poster.image = Image::from_rgba8(pixels);
                    poster.has_image = true;
                    library.continuing.set_row_data(item, poster);
                }
            });
        }
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
        let rows: Vec<Poster> = saved
            .iter()
            .enumerate()
            .map(|(item, saved)| {
                let old = had
                    .iter()
                    .find(|old| old.has_image && old.id == saved.title_id && old.kind == saved.kind);
                if old.is_none() {
                    if let Some(url) = &saved.poster {
                        wanted.push((item, url.clone()));
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
                }
            })
            .collect();
        self.watchlist.set_vec(rows);
        for (item, url) in wanted {
            self.fetch_image(url, POSTER_SIZE, move |library, pixels| {
                if generation != library.watchlist_generation.get() {
                    return;
                }
                if let Some(mut poster) = library.watchlist.row_data(item) {
                    poster.image = Image::from_rgba8(pixels);
                    poster.has_image = true;
                    library.watchlist.set_row_data(item, poster);
                }
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
            }
            Err(err) => eprintln!("haven: could not change the watchlist: {err}"),
        }
    }

    // ---- A title's page -------------------------------------------------

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
        self.set_streams_state(Some("Streams"), false, "");
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
            ..Detail::default()
        });
        ui.set_detail_open(true);

        let sources: Vec<String> = self
            .installed
            .borrow()
            .iter()
            .filter(|addon| addon.manifest.provides("meta", &kind, &id))
            .map(|addon| addon.url.clone())
            .collect();
        *self.open.borrow_mut() =
            Some(OpenTitle { kind: kind.clone(), id: id.clone(), meta: None, wanted });
        if sources.is_empty() {
            return self.detail_loaded(
                generation,
                Err("None of your add-ons has details for this title.".into()),
            );
        }
        let addons = self.addons.clone();
        self.tasks.spawn(async move {
            // The first add-on that knows the title wins.
            let mut found = Err("None of your add-ons has details for this title.".to_owned());
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
        let meta = match found {
            Ok(meta) => meta,
            Err(err) => {
                self.update_detail(|detail| {
                    detail.loading = false;
                    detail.error = err.into();
                });
                // A film can still have streams without details.
                if kind != "series" {
                    self.load_streams(&kind, &id, "Streams");
                }
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
        let pictures = [
            (meta.poster.clone().filter(|_| needs_poster), POSTER_SIZE, DetailImage::Poster),
            (meta.background.clone(), BACKGROUND_SIZE, DetailImage::Background),
        ];
        for (url, size, which) in pictures {
            if let Some(url) = url {
                self.fetch_image(url, size, move |library, pixels| {
                    library.detail_image_loaded(generation, which, pixels);
                });
            }
        }

        let seasons = meta.seasons();
        let has_videos = !meta.videos.is_empty();
        let all: Vec<EpisodeRow> = meta.videos.iter().map(episode_row).collect();
        // The episode a Continue Watching card asked for, if it exists.
        let wanted = self.open.borrow_mut().as_mut().and_then(|open| open.wanted.take());
        let wanted = wanted.and_then(|id| meta.videos.iter().find(|video| video.id == id).cloned());
        if let Some(open) = self.open.borrow_mut().as_mut() {
            open.meta = Some(meta);
        }
        if !has_videos {
            self.load_streams(&kind, &id, "Streams");
        } else {
            self.set_streams_state(Some("Streams"), false, "Choose an episode to see its streams.");
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
        let rows: Vec<EpisodeRow> = match self.open.borrow().as_ref().and_then(|o| o.meta.as_ref()) {
            Some(meta) => meta.episodes(season.max(0) as u32).into_iter().map(episode_row).collect(),
            None => return,
        };
        self.episodes.set_vec(rows);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_season(season);
        }
    }

    fn open_episode(&self, video_id: &str) {
        let Some(kind) = self.open.borrow().as_ref().map(|open| open.kind.clone()) else {
            return;
        };
        let mut heading = "Streams".to_owned();
        let mut detail = String::new();
        let season = self.ui.upgrade().map(|ui| ui.get_season()).filter(|_| self.seasons.row_count() > 0);
        for row in 0..self.episodes.row_count() {
            let Some(mut episode) = self.episodes.row_data(row) else { continue };
            let selected = episode.id == video_id;
            if selected {
                heading = format!("Streams for {} {}", episode.number, episode.title);
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
        if let Some(listed) = self.listed.borrow_mut().as_mut() {
            listed.detail = detail;
        }
    }

    fn clear_streams(&self) {
        self.streams.set_vec(Vec::new());
        self.stream_addresses.borrow_mut().clear();
        *self.listed.borrow_mut() = None;
        self.show_resume_note();
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

    /// Plays the stream in that row of the list, if Haven can.
    fn play_stream(&self, row: i32) {
        let Some(address) =
            self.stream_addresses.borrow().get(row.max(0) as usize).cloned().flatten()
        else {
            return;
        };
        let Some(ui) = self.ui.upgrade() else { return };
        let open = self.open.borrow().as_ref().map(|open| {
            (
                open.kind.clone(),
                open.id.clone(),
                open.meta.as_ref().and_then(|meta| meta.poster.clone()),
            )
        });
        let listed = self
            .listed
            .borrow()
            .as_ref()
            .map(|listed| (listed.video_id.clone(), listed.detail.clone()));
        let (Some((kind, title_id, poster)), Some((video_id, detail))) = (open, listed) else {
            return;
        };
        let request = PlayRequest {
            address,
            title: ui.get_detail().title.to_string(),
            detail,
            kind,
            title_id,
            video_id,
            poster,
            remember: true,
        };
        if let Some(play) = self.player.borrow().as_ref() {
            play(request);
        }
    }

    /// Asks every add-on that offers streams for this film or episode.
    fn load_streams(&self, kind: &str, video_id: &str, heading: &str) {
        let generation = self.stream_generation.get() + 1;
        self.stream_generation.set(generation);
        self.streams.set_vec(Vec::new());
        self.stream_addresses.borrow_mut().clear();
        *self.listed.borrow_mut() =
            Some(Listed { video_id: video_id.to_owned(), detail: String::new() });
        self.show_resume_note();

        let sources: Vec<(String, String)> = self
            .installed
            .borrow()
            .iter()
            .filter(|addon| addon.manifest.provides("stream", kind, video_id))
            .map(|addon| (addon.url.clone(), addon.manifest.name.clone()))
            .collect();
        self.streams_pending.set(sources.len());
        if sources.is_empty() {
            return self.set_streams_state(
                Some(heading),
                false,
                "None of your add-ons offers streams for this.",
            );
        }
        self.set_streams_state(Some(heading), true, "");
        for (url, name) in sources {
            let (addons, kind, video_id) = (self.addons.clone(), kind.to_owned(), video_id.to_owned());
            self.tasks.spawn(async move {
                let streams =
                    addons.streams(&url, &kind, &video_id).await.map_err(|err| err.to_string());
                on_ui(move |library| library.streams_loaded(generation, &name, streams));
            });
        }
    }

    fn streams_loaded(&self, generation: u64, source: &str, streams: Result<Vec<Stream>, String>) {
        if generation != self.stream_generation.get() {
            return;
        }
        match streams {
            Ok(streams) => {
                let can_play = self.player.borrow().is_some();
                for stream in &streams {
                    if let Some(mut row) = stream_row(source, stream) {
                        // Only plain web addresses reach the player.
                        let address = stream
                            .url
                            .as_deref()
                            .filter(|_| can_play && stream.kind() == Some(StreamKind::Direct))
                            .and_then(web_address)
                            .map(str::to_owned);
                        row.playable = address.is_some();
                        self.stream_addresses.borrow_mut().push(address);
                        self.streams.push(row);
                    }
                }
            }
            Err(err) => eprintln!("haven: {source} gave no streams: {err}"),
        }
        let pending = self.streams_pending.get().saturating_sub(1);
        self.streams_pending.set(pending);
        if pending == 0 {
            let note = if self.streams.row_count() == 0 { "No streams found." } else { "" };
            self.set_streams_state(None, false, note);
        }
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
    }
}

fn stream_row(source: &str, stream: &Stream) -> Option<StreamRow> {
    let kind = match stream.kind()? {
        StreamKind::Direct => "Direct",
        StreamKind::External => "Web page",
        StreamKind::YouTube => "YouTube",
        StreamKind::Torrent => "Torrent",
    };
    // Add-ons often spread these over several lines; a row has one.
    let one_line = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(StreamRow {
        source: source.into(),
        name: one_line(stream.name.as_deref().unwrap_or("Stream")).into(),
        detail: one_line(stream.details().unwrap_or_default()).into(),
        kind: kind.into(),
        playable: false,
    })
}

/// Downloads a picture and shrinks it to fit `size`. `None` on any failure.
async fn download_image(http: &reqwest::Client, url: &str, size: (u32, u32)) -> Option<Pixels> {
    let bytes = http::get_bytes(http, url, IMAGE_BYTES).await.ok()?;
    tokio::task::spawn_blocking(move || {
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .ok()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8000);
        limits.max_image_height = Some(8000);
        reader.limits(limits);
        let small = reader.decode().ok()?.thumbnail(size.0, size.1).into_rgba8();
        Some(SharedPixelBuffer::clone_from_slice(small.as_raw(), small.width(), small.height()))
    })
    .await
    .ok()?
}

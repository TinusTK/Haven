//! The Explore page: one grid of posters to browse everything there is,
//! a source at a time. The sources are the built-in TMDB catalogue, each
//! library of each media server, and each catalogue of each add-on.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use haven_titles::{Catalog, MetaPreview};
use haven_metadata::Genre;
use slint::{Image, Model, ModelRc, SharedString, VecModel};

use super::{Library, POSTER_SIZE, on_ui};
use crate::{AppWindow, Poster};

/// Titles asked for at a time from a media server.
const SERVER_PAGE: u32 = 60;
/// TMDB hands out 20 titles a page; two pages fill a screen.
const TMDB_PAGES_AT_ONCE: u32 = 2;

/// Somewhere to browse.
#[derive(Clone)]
enum Source {
    Tmdb,
    Library { server: haven_servers::Server, library: haven_servers::Library },
    Addon { url: String, addon: String, catalog: Catalog },
}

impl Source {
    /// Where the source comes from: the heading it is listed under.
    fn origin(&self) -> String {
        match self {
            Source::Tmdb => "TMDB".to_owned(),
            Source::Library { server, .. } => server.name.clone(),
            Source::Addon { addon, .. } => addon.clone(),
        }
    }

    /// Its name under that heading.
    fn name(&self) -> String {
        match self {
            Source::Tmdb => "Films and series".to_owned(),
            Source::Library { library, .. } => library.name.clone(),
            Source::Addon { catalog, .. } => catalog.title(),
        }
    }

    /// Tells sources apart, whatever their place in the list.
    fn label(&self) -> String {
        format!("{}  ·  {}", self.name(), self.origin())
    }
}

#[derive(Default)]
pub(super) struct Explore {
    sources: RefCell<Vec<Source>>,
    /// The chosen source's label, to find it again when the list changes.
    chosen: RefCell<String>,
    /// 0 films, 1 series (TMDB only).
    kind: Cell<usize>,
    /// The chosen genre's place in `genres`; -1 for all.
    genre: Cell<i32>,
    /// TMDB's genres for the chosen kind, once they have arrived.
    tmdb_genres: RefCell<Vec<Genre>>,
    /// The chosen media server library's genres, once they have arrived.
    server_genres: RefCell<Vec<String>>,
    /// Words the listed sources must hold.
    words: RefCell<String>,
    items: Rc<VecModel<Poster>>,
    /// Goes up whenever the grid starts over.
    generation: Cell<u64>,
    /// Where the next page starts: a page number for TMDB, else how
    /// many titles to skip.
    next: Cell<u32>,
    loading: Cell<bool>,
    /// The first page of this source has not come yet.
    first: Cell<bool>,
    /// The grid shows the first page kept from last time.
    showing_kept: Cell<bool>,
    /// The name the source's first page is kept under.
    kept_name: RefCell<String>,
}

impl Library {
    pub(super) fn explore_items(&self) -> Rc<VecModel<Poster>> {
        self.explore.items.clone()
    }

    pub(super) fn start_explore(self: &Rc<Self>, ui: &AppWindow) {
        ui.set_explore_items(ModelRc::from(self.explore.items.clone()));
        ui.on_explore_pick({
            let library = self.clone();
            move |source, kind, genre| library.explore_pick(source, kind, genre)
        });
        ui.on_explore_filter({
            let library = self.clone();
            move |words| {
                *library.explore.words.borrow_mut() = words.to_string();
                library.show_explore_sources();
            }
        });
        ui.on_explore_load_more({
            let library = self.clone();
            move || library.explore_load()
        });
        self.rebuild_explore();
    }

    /// Lists the sources again (an add-on, a server or the TMDB key came
    /// or went). The chosen source stays chosen if it is still there.
    pub(super) fn rebuild_explore(&self) {
        let mut sources = Vec::new();
        if self.tmdb().is_some() {
            sources.push(Source::Tmdb);
        }
        for (server, library) in self.server_libraries.borrow().iter() {
            sources.push(Source::Library { server: server.clone(), library: library.clone() });
        }
        for addon in self.installed.borrow().iter() {
            for catalog in addon.catalogs.iter().filter(|catalog| catalog.browsable()) {
                sources.push(Source::Addon {
                    url: addon.url.clone(),
                    addon: addon.name.clone(),
                    catalog: catalog.clone(),
                });
            }
        }
        let kept = sources.iter().position(|source| source.label() == *self.explore.chosen.borrow());
        *self.explore.sources.borrow_mut() = sources;
        self.show_explore_sources();
        let Some(ui) = self.ui.upgrade() else { return };
        match kept {
            // Still there: only its place among the chips may have moved.
            Some(index) => ui.set_explore_source(index as i32),
            None => self.explore_pick(0, 0, -1),
        }
    }

    /// Fills the list of sources down the side: a heading for each place
    /// they come from, then its sources that match the filter.
    fn show_explore_sources(&self) {
        let words = self.explore.words.borrow().to_lowercase();
        let mut rows: Vec<crate::ExploreRow> = Vec::new();
        let mut heading = String::new();
        for (index, source) in self.explore.sources.borrow().iter().enumerate() {
            let label = source.label().to_lowercase();
            if !words.split_whitespace().all(|word| label.contains(word)) {
                continue;
            }
            if source.origin() != heading {
                heading = source.origin();
                rows.push(crate::ExploreRow { index: -1, label: heading.as_str().into(), header: true });
            }
            rows.push(crate::ExploreRow {
                index: index as i32,
                label: source.name().into(),
                header: false,
            });
        }
        if let Some(ui) = self.ui.upgrade() {
            ui.set_explore_count(self.explore.sources.borrow().len() as i32);
            ui.set_explore_sources(ModelRc::new(VecModel::from(rows)));
        }
    }

    /// Shows a source from its start, narrowed to a kind and a genre.
    fn explore_pick(&self, source: i32, kind: i32, genre: i32) {
        let explore = &self.explore;
        let generation = explore.generation.get() + 1;
        explore.generation.set(generation);
        explore.items.set_vec(Vec::new());
        explore.loading.set(false);
        let Some(ui) = self.ui.upgrade() else { return };
        let chosen = explore.sources.borrow().get(source.max(0) as usize).cloned();
        let Some(chosen) = chosen else {
            explore.chosen.borrow_mut().clear();
            ui.set_explore_kinds(ModelRc::default());
            ui.set_explore_genres(ModelRc::default());
            ui.set_explore_note("".into());
            ui.set_explore_loading(false);
            ui.set_explore_more(false);
            return;
        };
        let same_kind = *explore.chosen.borrow() == chosen.label() && explore.kind.get() == kind.max(0) as usize;
        *explore.chosen.borrow_mut() = chosen.label();
        explore.kind.set(kind.max(0) as usize);
        explore.genre.set(genre);
        ui.set_explore_source(source.max(0));
        ui.set_explore_kind(kind.max(0));
        ui.set_explore_genre(genre);
        ui.set_explore_note("".into());

        let texts = |names: Vec<String>| {
            ModelRc::new(VecModel::from(names.into_iter().map(SharedString::from).collect::<Vec<_>>()))
        };
        match &chosen {
            Source::Tmdb => {
                explore.next.set(1);
                ui.set_explore_kinds(texts(vec!["Films".into(), "Series".into()]));
                if !same_kind {
                    // Another kind has other genres: fetch them.
                    explore.tmdb_genres.borrow_mut().clear();
                    ui.set_explore_genres(ModelRc::default());
                    self.explore_fetch_genres(kind == 1);
                }
            }
            Source::Library { server, library } => {
                explore.next.set(0);
                ui.set_explore_kinds(ModelRc::default());
                if !same_kind {
                    explore.server_genres.borrow_mut().clear();
                    ui.set_explore_genres(ModelRc::default());
                    let (client, server, library, label) =
                        (self.server_client.clone(), server.clone(), library.id.clone(), chosen.label());
                    self.tasks.spawn(async move {
                        let genres = client.genres(&server, &library).await;
                        on_ui(move |library| {
                            // Still looking at the same source.
                            if *library.explore.chosen.borrow() != label || genres.is_empty() {
                                return;
                            }
                            let names: Vec<SharedString> =
                                genres.iter().map(|genre| genre.as_str().into()).collect();
                            *library.explore.server_genres.borrow_mut() = genres;
                            if let Some(ui) = library.ui.upgrade() {
                                ui.set_explore_genres(genre_choices(names));
                            }
                        });
                    });
                }
            }
            Source::Addon { catalog, .. } => {
                explore.next.set(0);
                ui.set_explore_kinds(ModelRc::default());
                ui.set_explore_genres(genre_choices(catalog.genres().iter().map(|genre| genre.as_str().into()).collect()));
                // A catalogue that only answers with a genre starts on its first.
                if genre < 0 && catalog.needs_input() {
                    explore.genre.set(0);
                    ui.set_explore_genre(0);
                }
            }
        }
        // The first page as it was last time shows at once; the fresh
        // one replaces it when it comes.
        let name = format!("explore|{}|{}|{}", chosen.label(), explore.kind.get(), explore.genre.get());
        explore.first.set(true);
        explore.showing_kept.set(false);
        if let Some(titles) = self.kept::<Vec<MetaPreview>>(&name) {
            if !titles.is_empty() {
                self.explore_add(generation, titles, &HashMap::new());
                explore.showing_kept.set(true);
            }
        }
        *explore.kept_name.borrow_mut() = name;
        self.explore_load();
    }

    fn explore_fetch_genres(&self, series: bool) {
        let Some(tmdb) = self.tmdb() else { return };
        self.tasks.spawn(async move {
            let Ok(genres) = tmdb.genres(series).await else { return };
            on_ui(move |library| {
                // Still the same visit to the same source and kind.
                if library.explore.kind.get() != usize::from(series)
                    || *library.explore.chosen.borrow() != Source::Tmdb.label()
                {
                    return;
                }
                let names: Vec<SharedString> =
                    genres.iter().map(|genre| genre.name.as_str().into()).collect();
                *library.explore.tmdb_genres.borrow_mut() = genres;
                if let Some(ui) = library.ui.upgrade() {
                    ui.set_explore_genres(genre_choices(names));
                }
            });
        });
    }

    /// Loads the source showing afresh (Explore pulled down). The titles
    /// on screen stay until the fresh first page replaces them.
    pub(super) fn refresh_explore(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let explore = &self.explore;
        let Some(source) = explore.sources.borrow().iter().position(|s| s.label() == *explore.chosen.borrow()) else {
            return;
        };
        ui.set_explore_refreshing(true);
        // The grid as it is becomes the page shown until the fresh one comes.
        let on_screen: Vec<MetaPreview> = explore
            .items
            .iter()
            .map(|poster| MetaPreview {
                id: poster.id.to_string(),
                kind: poster.kind.to_string(),
                name: poster.title.to_string(),
                poster: None,
                release_info: Some(poster.year.to_string()).filter(|year| !year.is_empty()),
                rating: Some(poster.rating.to_string()).filter(|r| !r.is_empty()),
            })
            .collect();
        let images: HashMap<String, Image> = explore
            .items
            .iter()
            .filter(|poster| poster.has_image)
            .map(|poster| (poster.id.to_string(), poster.image.clone()))
            .collect();
        let (kind, genre) = (explore.kind.get() as i32, explore.genre.get());
        // Forget the source so it is picked as new (its genres stay).
        self.explore_pick(source as i32, kind, genre);
        if !on_screen.is_empty() && !explore.showing_kept.get() {
            explore.items.set_vec(Vec::new());
            self.explore_add(explore.generation.get(), on_screen, &images);
            explore.showing_kept.set(true);
        } else if explore.showing_kept.get() {
            // Kept from last time: put the pictures on screen back.
            for at in 0..explore.items.row_count() {
                if let Some(mut poster) = explore.items.row_data(at) {
                    if let Some(image) = images.get(poster.id.as_str()).filter(|_| !poster.has_image) {
                        poster.image = image.clone();
                        poster.has_image = true;
                        explore.items.set_row_data(at, poster);
                    }
                }
            }
        }
    }

    /// Fetches the next page of the chosen source and adds it to the grid.
    fn explore_load(&self) {
        let explore = &self.explore;
        if explore.loading.get() {
            return;
        }
        let Some(source) = explore
            .sources
            .borrow()
            .iter()
            .find(|source| source.label() == *explore.chosen.borrow())
            .cloned()
        else {
            return;
        };
        let generation = explore.generation.get();
        let (next, genre) = (explore.next.get(), explore.genre.get());
        explore.loading.set(true);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_explore_loading(true);
        }
        let (addons, servers, tmdb) = (self.catalogues.clone(), self.server_client.clone(), self.tmdb());
        let tmdb_genre = usize::try_from(genre)
            .ok()
            .and_then(|at| explore.tmdb_genres.borrow().get(at).map(|genre| genre.id));
        let series = explore.kind.get() == 1;
        let server_genre =
            usize::try_from(genre).ok().and_then(|at| explore.server_genres.borrow().get(at).cloned());
        self.tasks.spawn(async move {
            // The titles, where the page after them starts, and whether
            // there may be one.
            let page: Result<(Vec<MetaPreview>, u32, bool), String> = match &source {
                Source::Tmdb => match tmdb {
                    Some(tmdb) => {
                        let mut titles = Vec::new();
                        let mut failed = None;
                        for page in next..next + TMDB_PAGES_AT_ONCE {
                            match tmdb.discover(series, tmdb_genre, page).await {
                                Ok(found) => titles.extend(found),
                                Err(err) => {
                                    failed = Some(err.to_string());
                                    break;
                                }
                            }
                        }
                        match failed {
                            Some(err) if titles.is_empty() => Err(err),
                            _ => {
                                let more = titles.len() as u32 >= 20 * TMDB_PAGES_AT_ONCE;
                                Ok((titles, next + TMDB_PAGES_AT_ONCE, more))
                            }
                        }
                    }
                    None => Err("there is no TMDB key".to_owned()),
                },
                Source::Library { server, library } => servers
                    .page(server, &library.id, server_genre.as_deref(), next, SERVER_PAGE)
                    .await
                    .map(|titles| {
                        let more = titles.len() as u32 >= SERVER_PAGE;
                        (titles, next + SERVER_PAGE, more)
                    })
                    .map_err(|err| err.to_string()),
                Source::Addon { url, catalog, .. } => {
                    let genre = usize::try_from(genre).ok().and_then(|at| catalog.genres().get(at));
                    addons
                        .catalog_page(url, catalog, genre.map(String::as_str), next as usize)
                        .await
                        .map(|titles| {
                            let count = titles.len() as u32;
                            (titles, next + count, catalog.pages() && count > 0)
                        })
                        .map_err(|err| err.to_string())
                }
            };
            on_ui(move |library| library.explore_loaded(generation, page));
        });
    }

    fn explore_loaded(&self, generation: u64, page: Result<(Vec<MetaPreview>, u32, bool), String>) {
        let explore = &self.explore;
        if generation != explore.generation.get() {
            return;
        }
        explore.loading.set(false);
        let Some(ui) = self.ui.upgrade() else { return };
        ui.set_explore_loading(false);
        let first = explore.first.replace(false);
        if first {
            ui.set_explore_refreshing(false);
        }
        let (titles, next, more) = match page {
            Ok(page) => page,
            // The kept page stays when the fresh one cannot be had.
            Err(_) if first && explore.showing_kept.get() => {
                explore.showing_kept.set(false);
                ui.set_explore_more(false);
                return;
            }
            Err(err) => {
                ui.set_explore_more(false);
                return ui.set_explore_note(format!("Could not load this: {err}.").into());
            }
        };
        explore.next.set(next);
        let mut on_hand = HashMap::new();
        if first {
            self.keep(explore.kept_name.borrow().clone(), titles.clone());
            // The fresh first page takes the kept one's place; pictures
            // already on screen stay.
            if explore.showing_kept.replace(false) {
                for poster in explore.items.iter().filter(|poster| poster.has_image) {
                    on_hand.insert(poster.id.to_string(), poster.image.clone());
                }
                explore.items.set_vec(Vec::new());
            }
        }
        let added = self.explore_add(generation, titles, &on_hand);
        drop(ui);
        let Some(ui) = self.ui.upgrade() else { return };
        // A page of nothing new means the end, whatever the source says.
        ui.set_explore_more(more && added > 0);
        ui.set_explore_note(if explore.items.row_count() == 0 { "Nothing here." } else { "" }.into());
    }

    /// Adds titles to the grid (each once) and fetches their posters,
    /// unless `on_hand` has them. Returns how many were added.
    fn explore_add(&self, generation: u64, titles: Vec<MetaPreview>, on_hand: &HashMap<String, Image>) -> usize {
        let explore = &self.explore;
        let mut added = 0;
        for title in haven_titles::without_repeats(titles) {
            // Pages can overlap, and the same film can be listed again
            // on a later page under another id.
            let key = haven_titles::title_key(&title.kind, &title.name, title.release_info.as_deref());
            let known = explore.items.iter().any(|known| {
                (known.id == title.id.as_str() && known.kind == title.kind.as_str())
                    || haven_titles::title_key(
                        known.kind.as_str(),
                        known.title.as_str(),
                        Some(known.year.as_str()).filter(|year| !year.is_empty()),
                    ) == key
            });
            if known {
                continue;
            }
            let image = on_hand.get(&title.id).cloned();
            explore.items.push(Poster {
                id: title.id.as_str().into(),
                kind: title.kind.as_str().into(),
                title: title.name.as_str().into(),
                year: title.release_info.as_deref().unwrap_or_default().into(),
                has_image: image.is_some(),
                image: image.unwrap_or_default(),
                video: "".into(),
                progress: 0.0,
                watched: self.film_watched(&title.kind, &title.id),
                rating: title.rating.as_deref().unwrap_or_default().into(),
                rank: 0,
            });
            added += 1;
            if on_hand.contains_key(&title.id) {
                continue;
            }
            if let Some(url) = title.poster {
                let id = title.id.clone();
                self.fetch_image(url, POSTER_SIZE, move |library, pixels| {
                    let explore = &library.explore;
                    if generation != explore.generation.get() {
                        return;
                    }
                    // Found by id: a kept page may have been replaced meanwhile.
                    let Some(item) = explore.items.iter().position(|poster| poster.id.as_str() == id) else { return };
                    if let Some(mut poster) = explore.items.row_data(item) {
                        poster.image = Image::from_rgba8(pixels);
                        poster.has_image = true;
                        explore.items.set_row_data(item, poster);
                    }
                });
            }
        }
        added
    }
}

/// The genres as the chooser lists them: "All genres" first, which
/// stands for no genre (-1). Empty when there are none to choose from.
fn genre_choices(names: Vec<SharedString>) -> ModelRc<SharedString> {
    if names.is_empty() {
        return ModelRc::default();
    }
    let mut choices: Vec<SharedString> = vec!["All genres".into()];
    choices.extend(names);
    ModelRc::new(VecModel::from(choices))
}

//! Live TV and Radio: the user's sources (M3U playlists and Xtream Codes
//! accounts), the channels in them, the categories down the side of the
//! page, and the user's own Favourites groups. Network work runs off the
//! UI thread and comes back through [`on_ui`].

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Duration};

use haven_core::{
    http,
    settings::{SwitchedOff, switch},
    storage::{Store, TvSource},
};
use haven_livetv::{
    Channel, XtreamAccount, groups, parse_m3u, source_name, to_m3u, xtream_channels,
    xtream_sign_in,
};
use haven_player::source::stream_address;
use slint::{ComponentHandle, ModelRc, VecModel};
use tokio::runtime::Handle;

use crate::{AppWindow, CategoryRow, ChannelRow, FavPick, SourceRow, library::PlayRequest};

/// Playlists can be long, but not endless.
const PLAYLIST_BYTES: usize = 32 * 1024 * 1024;
const FAVOURITES: &str = "tv_favourites";
/// How often Xtream Codes accounts are asked whether they still answer.
const CHECK_EVERY: Duration = Duration::from_secs(60);
const CHECK_WAIT: Duration = Duration::from_secs(8);
/// A source's state in Settings: not shown (a playlist, or switched
/// off), being checked, online, offline.
const NO_STATE: i32 = 0;
const CHECKING: i32 = 1;
const ONLINE: i32 = 2;
const OFFLINE: i32 = 3;
/// How a category is named in the page's "chosen" property: every
/// channel, one of the playlist's groups, or a Favourites group.
const ALL: &str = "";
const GROUP: &str = "g:";
const FAVOURITE: &str = "f:";

thread_local! {
    static CURRENT: RefCell<Option<Rc<LiveTv>>> = const { RefCell::new(None) };
}

/// Runs `work` on the UI thread with the live TV state, from any thread.
fn on_ui(work: impl FnOnce(&Rc<LiveTv>) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        let live = CURRENT.with(|current| current.borrow().clone());
        if let Some(live) = live {
            work(&live);
        }
    });
}

/// One of the user's own groups of channels. A channel is known by the
/// address it plays from.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Favourites {
    name: String,
    channels: Vec<String>,
}

/// Television or radio: its channels and what the filter shows of them.
#[derive(Default)]
struct Board {
    all: RefCell<Vec<Channel>>,
    shown: Rc<VecModel<ChannelRow>>,
    categories: Rc<VecModel<CategoryRow>>,
    /// The chosen category's key; empty for all channels.
    category: RefCell<String>,
    /// Words the channels' names must hold.
    words: RefCell<String>,
    /// Words the categories' names must hold.
    category_words: RefCell<String>,
}

pub struct LiveTv {
    ui: slint::Weak<AppWindow>,
    store: Rc<Store>,
    tasks: Handle,
    http: reqwest::Client,
    tv: Board,
    radio: Board,
    favourites: RefCell<Vec<Favourites>>,
    /// The address of the channel whose Favourites groups are being chosen.
    picking: RefCell<Option<String>>,
    /// Whether each Xtream Codes account answers, by its source address.
    states: RefCell<HashMap<String, i32>>,
    timer: slint::Timer,
    /// Plays a channel; unset in a build without the player.
    player: RefCell<Option<Box<dyn Fn(PlayRequest)>>>,
}

impl LiveTv {
    /// Shows the channels of the stored sources, then fetches each source
    /// again in the background.
    pub fn start(
        ui: &AppWindow,
        store: Rc<Store>,
        tasks: Handle,
    ) -> Result<Rc<Self>, http::HttpError> {
        let favourites = store.get::<Vec<Favourites>>(FAVOURITES).ok().flatten().unwrap_or_default();
        let live = Rc::new(Self {
            ui: ui.as_weak(),
            store,
            tasks,
            http: http::client()?,
            tv: Board::default(),
            radio: Board::default(),
            favourites: RefCell::new(favourites),
            picking: RefCell::default(),
            states: RefCell::default(),
            timer: slint::Timer::default(),
            player: RefCell::default(),
        });
        CURRENT.with(|current| *current.borrow_mut() = Some(live.clone()));

        ui.set_tv_channels(ModelRc::from(live.tv.shown.clone()));
        ui.set_tv_categories(ModelRc::from(live.tv.categories.clone()));
        ui.set_radio_channels(ModelRc::from(live.radio.shown.clone()));
        ui.set_radio_categories(ModelRc::from(live.radio.categories.clone()));

        macro_rules! connect {
            ($on:ident, |$($arg:ident),*| $call:expr) => {
                ui.$on({
                    let live = live.clone();
                    move |$($arg),*| {
                        let live = &live;
                        $call(live)
                    }
                });
            };
        }
        connect!(on_add_tv_source, |address| |l: &Rc<Self>| l.add(address.as_str()));
        connect!(on_add_xtream, |server, user, password| |l: &Rc<Self>| l
            .add_xtream(server.as_str(), user.as_str(), password.as_str()));
        connect!(on_remove_tv_source, |url| |l: &Rc<Self>| l.remove(url.as_str()));
        connect!(on_set_tv_source_enabled, |url, on| |l: &Rc<Self>| l.set_enabled(url.as_str(), on));
        connect!(on_filter_channels, |radio, words, category| |l: &Rc<Self>| l
            .filter(radio, words.as_str(), category.as_str()));
        connect!(on_filter_categories, |radio, words| |l: &Rc<Self>| l
            .filter_categories(radio, words.as_str()));
        connect!(on_play_channel, |radio, index| |l: &Rc<Self>| l.play(radio, index));
        connect!(on_fav_pick, |radio, index| |l: &Rc<Self>| l.pick_favourites(radio, index));
        connect!(on_fav_toggle, |name| |l: &Rc<Self>| l.toggle_favourite(name.as_str()));
        connect!(on_fav_new_group, |name| |l: &Rc<Self>| l.new_group(name.as_str()));
        connect!(on_fav_delete_group, |name| |l: &Rc<Self>| l.delete_group(name.as_str()));
        connect!(on_fav_close, | | |l: &Rc<Self>| l.close_picker());

        live.rebuild();
        let off = live.switched_off();
        for source in live.sources() {
            if !off.contains(&source.url) {
                live.fetch(source.url, false);
            }
        }
        let weak = Rc::downgrade(&live);
        live.timer.start(slint::TimerMode::Repeated, CHECK_EVERY, move || {
            if let Some(live) = weak.upgrade() {
                live.check_accounts();
            }
        });
        Ok(live)
    }

    /// Gives live TV a player; from then on channels can be played.
    #[cfg_attr(not(feature = "player"), allow(dead_code))]
    pub fn set_player(&self, play: impl Fn(PlayRequest) + 'static) {
        *self.player.borrow_mut() = Some(Box::new(play));
        if let Some(ui) = self.ui.upgrade() {
            ui.set_can_play_channels(true);
        }
    }

    fn board(&self, radio: bool) -> &Board {
        if radio { &self.radio } else { &self.tv }
    }

    fn sources(&self) -> Vec<TvSource> {
        self.store.tv_sources().unwrap_or_else(|err| {
            eprintln!("haven: could not read the TV sources: {err}");
            Vec::new()
        })
    }

    /// The sources the user has switched off, by address.
    fn switched_off(&self) -> Vec<String> {
        SwitchedOff::load(&self.store).unwrap_or_default().tv_sources
    }

    fn set_status(&self, text: &str, is_error: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_tv_status(text.into());
            ui.set_tv_status_is_error(is_error);
        }
    }

    fn set_xtream_status(&self, text: &str, is_error: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_xtream_status(text.into());
            ui.set_xtream_status_is_error(is_error);
        }
    }

    // ---- Sources --------------------------------------------------------

    /// Fills the list of sources in Settings.
    fn show_sources(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let off = self.switched_off();
        let states = self.states.borrow();
        let rows: Vec<SourceRow> = self
            .sources()
            .iter()
            .map(|source| {
                let account = XtreamAccount::from_source_url(&source.url);
                let enabled = !off.contains(&source.url);
                SourceRow {
                    url: source.url.as_str().into(),
                    name: source.name.as_str().into(),
                    channels: playable(parse_m3u(&source.playlist)).len() as i32,
                    // An account's address holds its password: never shown.
                    detail: match &account {
                        Some(account) => format!("Xtream Codes account of {}", account.username),
                        None => source.url.clone(),
                    }
                    .into(),
                    enabled,
                    state: match account {
                        Some(_) if enabled => states.get(&source.url).copied().unwrap_or(CHECKING),
                        _ => NO_STATE,
                    },
                }
            })
            .collect();
        ui.set_tv_sources(ModelRc::new(VecModel::from(rows)));
    }

    fn set_state(&self, url: &str, state: i32) {
        if self.states.borrow_mut().insert(url.to_owned(), state) != Some(state) {
            self.show_sources();
        }
    }

    /// Asks every Xtream Codes account that is switched on whether its
    /// server answers and still accepts the sign-in.
    fn check_accounts(&self) {
        let off = self.switched_off();
        for source in self.sources() {
            let Some(account) = XtreamAccount::from_source_url(&source.url) else { continue };
            if off.contains(&source.url) {
                continue;
            }
            let http = self.http.clone();
            self.tasks.spawn(async move {
                let asked = tokio::time::timeout(
                    CHECK_WAIT,
                    http::get_bytes(&http, &account.api(None), PLAYLIST_BYTES),
                )
                .await;
                let online = matches!(asked, Ok(Ok(answer))
                    if xtream_sign_in(&String::from_utf8_lossy(&answer)).is_ok());
                on_ui(move |live| {
                    live.set_state(&source.url, if online { ONLINE } else { OFFLINE });
                });
            });
        }
    }

    /// Switches a source on or off. Off, it stays in the list but its
    /// channels are not shown and it is not fetched.
    fn set_enabled(&self, url: &str, on: bool) {
        let mut off = SwitchedOff::load(&self.store).unwrap_or_default();
        switch(&mut off.tv_sources, url.to_owned(), on);
        if let Err(err) = off.save(&self.store) {
            return self.set_xtream_status(&format!("Could not save that: {err}"), true);
        }
        self.states.borrow_mut().remove(url);
        self.rebuild();
        if on {
            self.fetch(url.to_owned(), false);
        }
    }

    /// Reads every stored playlist again and refills both pages.
    fn rebuild(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let off = self.switched_off();
        let (mut tv, mut radio) = (Vec::new(), Vec::new());
        for source in self.sources() {
            if off.contains(&source.url) {
                continue;
            }
            for channel in playable(parse_m3u(&source.playlist)) {
                if channel.radio { radio.push(channel) } else { tv.push(channel) }
            }
        }
        self.show_sources();
        ui.set_tv_total(tv.len() as i32);
        ui.set_radio_total(radio.len() as i32);
        for (is_radio, channels) in [(false, tv), (true, radio)] {
            *self.board(is_radio).all.borrow_mut() = channels;
            self.show_categories(is_radio);
            self.refill(is_radio);
        }
    }

    // ---- Categories and channels ------------------------------------------

    /// Fills the list down the side: the Favourites groups first, then
    /// every channel, then the playlists' own groups that match the
    /// category filter. A chosen category that is gone falls back to all.
    fn show_categories(&self, radio: bool) {
        let board = self.board(radio);
        let all = board.all.borrow();
        let words = board.category_words.borrow().to_lowercase();
        let matches = |name: &str| {
            let name = name.to_lowercase();
            words.split_whitespace().all(|word| name.contains(word))
        };
        let mut rows = Vec::new();
        // Favourites always come first, whatever the filter says.
        for group in self.favourites.borrow().iter() {
            let count = all.iter().filter(|channel| group.channels.contains(&channel.address)).count();
            // Radio only lists the groups that hold stations.
            if !radio || count > 0 {
                rows.push(CategoryRow {
                    key: format!("{FAVOURITE}{}", group.name).into(),
                    label: group.name.as_str().into(),
                    count: count as i32,
                    favourite: true,
                });
            }
        }
        rows.push(CategoryRow {
            key: ALL.into(),
            label: if radio { "All stations" } else { "All channels" }.into(),
            count: all.len() as i32,
            favourite: false,
        });
        let mut counts: HashMap<&str, i32> = HashMap::new();
        for group in all.iter().filter_map(|channel| channel.group.as_deref()) {
            *counts.entry(group).or_default() += 1;
        }
        for name in groups(&all) {
            if matches(&name) {
                rows.push(CategoryRow {
                    key: format!("{GROUP}{name}").into(),
                    count: counts.get(name.as_str()).copied().unwrap_or(0),
                    label: name.into(),
                    favourite: false,
                });
            }
        }
        // The chosen one must still exist (whether or not the filter shows it).
        let chosen = board.category.borrow().clone();
        let exists = match chosen.as_str() {
            ALL => true,
            key => match key.strip_prefix(FAVOURITE) {
                Some(name) => self.favourites.borrow().iter().any(|group| group.name == name),
                None => key
                    .strip_prefix(GROUP)
                    .is_some_and(|name| all.iter().any(|channel| channel.group.as_deref() == Some(name))),
            },
        };
        drop(all);
        if !exists {
            board.category.borrow_mut().clear();
            if let Some(ui) = self.ui.upgrade() {
                if radio { ui.set_radio_category("".into()) } else { ui.set_tv_category("".into()) }
            }
        }
        board.categories.set_vec(rows);
    }

    /// Shows the channels that pass the board's filter.
    fn refill(&self, radio: bool) {
        let board = self.board(radio);
        let (category, words) = (board.category.borrow(), board.words.borrow().to_lowercase());
        let favourites = self.favourites.borrow();
        let starred =
            |address: &str| favourites.iter().any(|group| group.channels.iter().any(|a| a == address));
        let in_category = |channel: &Channel| match category.as_str() {
            ALL => true,
            key => match key.strip_prefix(FAVOURITE) {
                Some(name) => favourites
                    .iter()
                    .any(|group| group.name == name && group.channels.contains(&channel.address)),
                None => key.strip_prefix(GROUP).is_some_and(|name| channel.group.as_deref() == Some(name)),
            },
        };
        let rows: Vec<ChannelRow> = board
            .all
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, channel)| {
                let name = channel.name.to_lowercase();
                in_category(channel) && words.split_whitespace().all(|word| name.contains(word))
            })
            .map(|(index, channel)| ChannelRow {
                index: index as i32,
                name: channel.name.as_str().into(),
                group: channel.group.as_deref().unwrap_or_default().into(),
                starred: starred(&channel.address),
            })
            .collect();
        board.shown.set_vec(rows);
    }

    fn filter(&self, radio: bool, words: &str, category: &str) {
        let board = self.board(radio);
        *board.words.borrow_mut() = words.to_owned();
        *board.category.borrow_mut() = category.to_owned();
        self.refill(radio);
    }

    fn filter_categories(&self, radio: bool, words: &str) {
        *self.board(radio).category_words.borrow_mut() = words.to_owned();
        self.show_categories(radio);
    }

    // ---- Favourites -------------------------------------------------------

    fn save_favourites(&self) {
        if let Err(err) = self.store.put(FAVOURITES, &*self.favourites.borrow()) {
            eprintln!("haven: could not save the favourites: {err}");
        }
        for radio in [false, true] {
            self.show_categories(radio);
            self.refill(radio);
        }
        self.show_picker();
    }

    /// Opens the chooser of Favourites groups for a channel.
    fn pick_favourites(&self, radio: bool, index: i32) {
        let Some(channel) = self.board(radio).all.borrow().get(index.max(0) as usize).cloned()
        else {
            return;
        };
        *self.picking.borrow_mut() = Some(channel.address);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_fav_channel(channel.name.into());
            ui.set_fav_new_name("".into());
            ui.set_fav_open(true);
        }
        self.show_picker();
    }

    /// Fills the chooser: every group, ticked if it holds the channel.
    fn show_picker(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let picking = self.picking.borrow();
        let picks: Vec<FavPick> = self
            .favourites
            .borrow()
            .iter()
            .map(|group| FavPick {
                name: group.name.as_str().into(),
                has: picking.as_ref().is_some_and(|address| group.channels.contains(address)),
            })
            .collect();
        ui.set_fav_picks(ModelRc::new(VecModel::from(picks)));
    }

    /// Puts the channel being chosen for into a group, or takes it out.
    fn toggle_favourite(&self, name: &str) {
        let Some(address) = self.picking.borrow().clone() else { return };
        if let Some(group) = self.favourites.borrow_mut().iter_mut().find(|group| group.name == name) {
            match group.channels.iter().position(|known| *known == address) {
                Some(at) => {
                    group.channels.remove(at);
                }
                None => group.channels.push(address),
            }
        }
        self.save_favourites();
    }

    /// Makes a new group. If a channel is being chosen for, it goes in.
    fn new_group(&self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let address = self.picking.borrow().clone();
        {
            let mut favourites = self.favourites.borrow_mut();
            if !favourites.iter().any(|group| group.name == name) {
                favourites.push(Favourites { name: name.to_owned(), channels: Vec::new() });
            }
            if let (Some(address), Some(group)) =
                (address, favourites.iter_mut().find(|group| group.name == name))
            {
                if !group.channels.contains(&address) {
                    group.channels.push(address);
                }
            }
        }
        if let Some(ui) = self.ui.upgrade() {
            ui.set_fav_new_name("".into());
        }
        self.save_favourites();
    }

    /// Deletes a group; its channels stay in the playlist.
    fn delete_group(&self, name: &str) {
        self.favourites.borrow_mut().retain(|group| group.name != name);
        self.save_favourites();
    }

    fn close_picker(&self) {
        *self.picking.borrow_mut() = None;
        if let Some(ui) = self.ui.upgrade() {
            ui.set_fav_open(false);
        }
    }

    // ---- Adding and fetching ----------------------------------------------

    /// Adds the playlist at the address the user gave.
    fn add(&self, address: &str) {
        let url = match http::web_url(address) {
            Ok(url) => url.to_string(),
            Err(err) => return self.set_status(&format!("Could not add the source: {err}."), true),
        };
        if let Some(ui) = self.ui.upgrade() {
            ui.set_tv_busy(true);
        }
        self.set_status("Fetching the channel list…", false);
        self.fetch(url, true);
    }

    /// Adds an Xtream Codes account from what the user typed in Settings.
    fn add_xtream(&self, server: &str, user: &str, password: &str) {
        let Some(account) = XtreamAccount::new(server, user, password) else {
            return self.set_xtream_status(
                "Could not add the account: give the server's web address, your user name and your password.",
                true,
            );
        };
        if let Some(ui) = self.ui.upgrade() {
            ui.set_xtream_busy(true);
        }
        self.set_xtream_status("Signing in and fetching the channel list…", false);
        self.fetch_xtream(account, true);
    }

    /// Signs in to an Xtream Codes account and fetches its live channels,
    /// which are then kept as playlist text like any other source's.
    fn fetch_xtream(&self, account: XtreamAccount, adding: bool) {
        let http = self.http.clone();
        self.tasks.spawn(async move {
            let text = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();
            let fetched = async {
                let info = http::get_bytes(&http, &account.api(None), PLAYLIST_BYTES)
                    .await
                    .map_err(|err| err.to_string())?;
                let info = xtream_sign_in(&text(info)).map_err(|err| err.to_string())?;
                // The groups' names are worth having but not worth failing for.
                let categories =
                    http::get_bytes(&http, &account.api(Some("get_live_categories")), PLAYLIST_BYTES)
                        .await
                        .map(text)
                        .unwrap_or_default();
                let streams =
                    http::get_bytes(&http, &account.api(Some("get_live_streams")), PLAYLIST_BYTES)
                        .await
                        .map_err(|err| err.to_string())?;
                let channels = xtream_channels(&account, &info, &categories, &text(streams))
                    .map_err(|err| err.to_string())?;
                Ok::<_, String>(to_m3u(&channels))
            }
            .await;
            let (url, name) = (account.source_url(), account.label());
            on_ui(move |live| live.fetched(&url, Some(name), adding, fetched));
        });
    }

    /// Downloads a source's playlist. `adding` is true when the user just
    /// asked for it (failures are then shown); false for the quiet refresh
    /// at start.
    fn fetch(&self, url: String, adding: bool) {
        if let Some(account) = XtreamAccount::from_source_url(&url) {
            return self.fetch_xtream(account, adding);
        }
        let http = self.http.clone();
        self.tasks.spawn(async move {
            let fetched = http::get_bytes(&http, &url, PLAYLIST_BYTES)
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|err| err.to_string());
            on_ui(move |live| live.fetched(&url, None, adding, fetched));
        });
    }

    /// A source's channel list has arrived, or failed to. `account` is
    /// the name to list it under when it is an Xtream Codes account.
    fn fetched(
        &self,
        url: &str,
        account: Option<String>,
        adding: bool,
        fetched: Result<String, String>,
    ) {
        let xtream = account.is_some();
        let say = |text: &str, is_error: bool| {
            if xtream { self.set_xtream_status(text, is_error) } else { self.set_status(text, is_error) }
        };
        if adding {
            if let Some(ui) = self.ui.upgrade() {
                if xtream { ui.set_xtream_busy(false) } else { ui.set_tv_busy(false) }
            }
        }
        let known = self.sources().iter().any(|source| source.url == url);
        // An account that answered is online; one that did not is offline.
        if xtream && (known || fetched.is_ok()) {
            self.states
                .borrow_mut()
                .insert(url.to_owned(), if fetched.is_ok() { ONLINE } else { OFFLINE });
        }
        let playlist = match fetched {
            Ok(playlist) => playlist,
            Err(err) => {
                if adding {
                    say(&format!("Could not add the {}: {err}.", if xtream { "account" } else { "source" }), true);
                } else {
                    eprintln!("haven: could not refresh {}: {err}", source_name(url));
                    self.show_sources();
                }
                return;
            }
        };
        let channels = playable(parse_m3u(&playlist));
        if channels.is_empty() {
            // Not a playlist (or an empty one): keep what was there before.
            if adding {
                say(
                    if xtream { "The account has no live channels." } else { "No channels were found at that address." },
                    true,
                );
            }
            return;
        }
        // A source removed while its refresh was on the way stays removed.
        if !adding && !known {
            return;
        }
        let saved = self.store.save_tv_source(&TvSource {
            url: url.to_owned(),
            name: account.unwrap_or_else(|| source_name(url)),
            playlist,
        });
        if let Err(err) = saved {
            return say(&format!("Could not save the source: {err}"), true);
        }
        self.rebuild();
        if adding {
            let stations = channels.iter().filter(|channel| channel.radio).count();
            let count = |n: usize, one: &str, many: &str| {
                if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
            };
            let mut found = vec![count(channels.len() - stations, "channel", "channels")];
            if stations > 0 {
                found.push(count(stations, "radio station", "radio stations"));
            }
            say(&format!("Added {}. They are on the Live TV tab.", found.join(" and ")), false);
            if let Some(ui) = self.ui.upgrade() {
                if xtream {
                    ui.set_xtream_server("".into());
                    ui.set_xtream_user("".into());
                    ui.set_xtream_password("".into());
                } else {
                    ui.set_tv_address("".into());
                }
            }
        }
    }

    fn remove(&self, url: &str) {
        if let Err(err) = self.store.remove_tv_source(url) {
            return self.set_xtream_status(&format!("Could not remove the source: {err}"), true);
        }
        let mut off = SwitchedOff::load(&self.store).unwrap_or_default();
        switch(&mut off.tv_sources, url.to_owned(), true);
        let _ = off.save(&self.store);
        self.states.borrow_mut().remove(url);
        self.rebuild();
        self.set_status("", false);
        self.set_xtream_status("Source removed.", false);
    }

    /// Plays a channel; `index` is its place among all of the board's
    /// channels (not among the filtered ones).
    fn play(&self, radio: bool, index: i32) {
        let Some(channel) = self.board(radio).all.borrow().get(index.max(0) as usize).cloned()
        else {
            return;
        };
        if let Some(play) = self.player.borrow().as_ref() {
            play(PlayRequest {
                address: channel.address,
                title: channel.name,
                detail: channel.group.unwrap_or_default(),
                kind: if radio { "radio" } else { "tv" }.to_owned(),
                title_id: String::new(),
                video_id: String::new(),
                poster: None,
                // A channel has no place to come back to.
                remember: false,
                source: String::new(),
                next: None,
            });
        }
    }
}

/// Keeps the channels whose address the player may open.
fn playable(channels: Vec<Channel>) -> Vec<Channel> {
    channels.into_iter().filter(|channel| stream_address(&channel.address).is_some()).collect()
}

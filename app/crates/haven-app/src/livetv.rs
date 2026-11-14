//! Live TV and Radio: the user's playlist sources, the channels in them,
//! and the filter over them. Network work runs off the UI thread and comes
//! back through [`on_ui`].

use std::{cell::RefCell, rc::Rc};

use haven_core::{
    http,
    storage::{Store, TvSource},
};
use haven_livetv::{Channel, groups, matches, parse_m3u, source_name};
use haven_player::source::stream_address;
use slint::{ComponentHandle, ModelRc, VecModel};
use tokio::runtime::Handle;

use crate::{AppWindow, ChannelRow, SourceRow, library::PlayRequest};

/// Playlists can be long, but not endless.
const PLAYLIST_BYTES: usize = 32 * 1024 * 1024;

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

/// Television or radio: its channels and what the filter shows of them.
#[derive(Default)]
struct Board {
    all: RefCell<Vec<Channel>>,
    shown: Rc<VecModel<ChannelRow>>,
    groups: Rc<VecModel<slint::SharedString>>,
    /// The chosen group; empty for all.
    group: RefCell<String>,
    words: RefCell<String>,
}

pub struct LiveTv {
    ui: slint::Weak<AppWindow>,
    store: Rc<Store>,
    tasks: Handle,
    http: reqwest::Client,
    tv: Board,
    radio: Board,
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
        let live = Rc::new(Self {
            ui: ui.as_weak(),
            store,
            tasks,
            http: http::client()?,
            tv: Board::default(),
            radio: Board::default(),
            player: RefCell::default(),
        });
        CURRENT.with(|current| *current.borrow_mut() = Some(live.clone()));

        ui.set_tv_channels(ModelRc::from(live.tv.shown.clone()));
        ui.set_tv_groups(ModelRc::from(live.tv.groups.clone()));
        ui.set_radio_channels(ModelRc::from(live.radio.shown.clone()));
        ui.set_radio_groups(ModelRc::from(live.radio.groups.clone()));

        ui.on_add_tv_source({
            let live = live.clone();
            move |address| live.add(address.as_str())
        });
        ui.on_remove_tv_source({
            let live = live.clone();
            move |url| live.remove(url.as_str())
        });
        ui.on_filter_channels({
            let live = live.clone();
            move |radio, words, group| live.filter(radio, words.as_str(), group.as_str())
        });
        ui.on_play_channel({
            let live = live.clone();
            move |radio, index| live.play(radio, index)
        });

        live.rebuild();
        for source in live.sources() {
            live.fetch(source.url, false);
        }
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

    fn set_status(&self, text: &str, is_error: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_tv_status(text.into());
            ui.set_tv_status_is_error(is_error);
        }
    }

    /// Reads every stored playlist again and refills both pages.
    fn rebuild(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut rows = Vec::new();
        let (mut tv, mut radio) = (Vec::new(), Vec::new());
        for source in self.sources() {
            let channels = playable(parse_m3u(&source.playlist));
            rows.push(SourceRow {
                url: source.url.as_str().into(),
                name: source.name.as_str().into(),
                channels: channels.len() as i32,
            });
            for channel in channels {
                if channel.radio { radio.push(channel) } else { tv.push(channel) }
            }
        }
        ui.set_tv_sources(ModelRc::new(VecModel::from(rows)));
        ui.set_tv_total(tv.len() as i32);
        ui.set_radio_total(radio.len() as i32);
        for (is_radio, channels) in [(false, tv), (true, radio)] {
            let board = self.board(is_radio);
            let names: Vec<slint::SharedString> =
                groups(&channels).into_iter().map(Into::into).collect();
            // A group that is gone cannot stay chosen.
            if !names.iter().any(|name| name.as_str() == board.group.borrow().as_str()) {
                board.group.borrow_mut().clear();
                if is_radio { ui.set_radio_group("".into()) } else { ui.set_tv_group("".into()) }
            }
            board.groups.set_vec(names);
            *board.all.borrow_mut() = channels;
            self.refill(is_radio);
        }
    }

    /// Shows the channels that pass the board's filter.
    fn refill(&self, radio: bool) {
        let board = self.board(radio);
        let (group, words) = (board.group.borrow(), board.words.borrow());
        let group = (!group.is_empty()).then_some(group.as_str());
        let rows: Vec<ChannelRow> = board
            .all
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, channel)| matches(channel, group, &words))
            .map(|(index, channel)| ChannelRow {
                index: index as i32,
                name: channel.name.as_str().into(),
                group: channel.group.as_deref().unwrap_or_default().into(),
            })
            .collect();
        board.shown.set_vec(rows);
    }

    fn filter(&self, radio: bool, words: &str, group: &str) {
        let board = self.board(radio);
        *board.words.borrow_mut() = words.to_owned();
        *board.group.borrow_mut() = group.to_owned();
        self.refill(radio);
    }

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

    /// Downloads a source's playlist. `adding` is true when the user just
    /// asked for it (failures are then shown); false for the quiet refresh
    /// at start.
    fn fetch(&self, url: String, adding: bool) {
        let http = self.http.clone();
        self.tasks.spawn(async move {
            let fetched = http::get_bytes(&http, &url, PLAYLIST_BYTES)
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|err| err.to_string());
            on_ui(move |live| live.fetched(&url, adding, fetched));
        });
    }

    fn fetched(&self, url: &str, adding: bool, fetched: Result<String, String>) {
        if adding {
            if let Some(ui) = self.ui.upgrade() {
                ui.set_tv_busy(false);
            }
        }
        let playlist = match fetched {
            Ok(playlist) => playlist,
            Err(err) => {
                if adding {
                    self.set_status(&format!("Could not add the source: {err}."), true);
                } else {
                    eprintln!("haven: could not refresh {}: {err}", source_name(url));
                }
                return;
            }
        };
        let channels = playable(parse_m3u(&playlist));
        if channels.is_empty() {
            // Not a playlist (or an empty one): keep what was there before.
            if adding {
                self.set_status("No channels were found at that address.", true);
            }
            return;
        }
        // A source removed while its refresh was on the way stays removed.
        if !adding && !self.sources().iter().any(|source| source.url == url) {
            return;
        }
        let saved = self.store.save_tv_source(&TvSource {
            url: url.to_owned(),
            name: source_name(url),
            playlist,
        });
        if let Err(err) = saved {
            return self.set_status(&format!("Could not save the source: {err}"), true);
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
            self.set_status(&format!("Added {}.", found.join(" and ")), false);
            if let Some(ui) = self.ui.upgrade() {
                ui.set_tv_address("".into());
            }
        }
    }

    fn remove(&self, url: &str) {
        if let Err(err) = self.store.remove_tv_source(url) {
            return self.set_status(&format!("Could not remove the source: {err}"), true);
        }
        self.rebuild();
        self.set_status("Source removed.", false);
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
            });
        }
    }
}

/// Keeps the channels whose address the player may open.
fn playable(channels: Vec<Channel>) -> Vec<Channel> {
    channels.into_iter().filter(|channel| stream_address(&channel.address).is_some()).collect()
}

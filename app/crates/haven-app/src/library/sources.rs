//! Where a film or episode can be watched: every version of it on every
//! media server that has it, and every stream the add-ons offer for it.
//! The user picks one from the list on the title's page.

use haven_addons::{Stream, StreamKind};
use haven_metadata::is_imdb_id;
use haven_player::source::web_address;
use haven_servers::{Wanted, split_id, title_id};
use slint::Model;

use haven_addons::Video;

use super::{Library, Listed, PlayRequest, Played, on_ui};
use crate::StreamRow;

/// The heading over the list.
pub(super) const HEADING: &str = "Play from your sources";

/// Places to watch shown per provider at first, and added by "Show more".
const STREAMS_SHOWN: i32 = 10;

/// What is behind one row of the list.
pub(super) struct StreamSource {
    /// The address the player opens; `None` if Haven cannot play the row.
    address: Option<String>,
    /// The media server item (`srv:…`) the row plays, if it is a
    /// server's row: the server is told how far that item was watched.
    server_item: Option<String>,
}

impl Library {
    pub(super) fn clear_streams(&self) {
        self.streams.set_vec(Vec::new());
        self.stream_sources.borrow_mut().clear();
        self.server_rows.set(0);
        self.show_stream_groups();
        *self.listed.borrow_mut() = None;
        self.show_resume_note();
    }

    /// Describes the film or episode with the id `video_id` of the open
    /// title, so that media servers can be asked whether they have it.
    fn wanted(&self, kind: &str, video_id: &str) -> Wanted {
        let open = self.open.borrow();
        let meta = open.as_ref().and_then(|open| open.meta.as_ref());
        let open_id = open.as_ref().map(|open| open.id.as_str()).unwrap_or_default();
        let name = match meta {
            Some(meta) => meta.name.clone(),
            None => self.ui.upgrade().map(|ui| ui.get_detail().title.to_string()).unwrap_or_default(),
        };
        // The IMDb id: named by the details, or the title's own id, or
        // the start of an episode id such as `tt123:1:2`.
        let imdb = meta
            .and_then(|meta| meta.imdb_id.clone())
            .or_else(|| meta.map(|meta| meta.id.clone()).filter(|id| is_imdb_id(id)))
            .or_else(|| Some(open_id.to_owned()).filter(|id| is_imdb_id(id)))
            .or_else(|| video_id.split(':').next().filter(|id| is_imdb_id(id)).map(str::to_owned));
        let tmdb = meta
            .and_then(|meta| meta.tmdb_id.clone())
            .or_else(|| haven_metadata::split_id(open_id).map(|(_, number)| number.to_string()));
        Wanted {
            series: kind == "series",
            name,
            year: meta.and_then(|meta| meta.release_info.clone()),
            imdb,
            tmdb,
            episode: meta
                .and_then(|meta| meta.videos.iter().find(|video| video.id == video_id))
                .and_then(|video| video.season.zip(video.episode)),
        }
    }

    /// Lists every place the film or episode can be watched: asks each
    /// media server whether it has it, and every add-on that offers
    /// streams for it.
    pub(super) fn load_streams(&self, kind: &str, video_id: &str, heading: &str) {
        let generation = self.stream_generation.get() + 1;
        self.stream_generation.set(generation);
        self.streams.set_vec(Vec::new());
        self.stream_sources.borrow_mut().clear();
        self.server_rows.set(0);
        self.show_stream_groups();
        *self.listed.borrow_mut() =
            Some(Listed { video_id: video_id.to_owned(), detail: String::new() });
        self.show_resume_note();

        let wanted = self.wanted(kind, video_id);
        // A series can only be matched down to an episode.
        let can_match = !wanted.name.trim().is_empty() && (!wanted.series || wanted.episode.is_some());
        let origin = self.server_item(video_id);
        // A title a server gathered under a TMDB or other number has no
        // sources under that name: it is looked up again, by its IMDb id.
        let renamed = self
            .open
            .borrow()
            .as_ref()
            .and_then(|open| split_id(&open.id).map(|(_, item)| haven_servers::wants_imdb(item)))
            .unwrap_or(false);
        // Where an IMDb id can be looked up by name when none is known:
        // the catalogues of this kind that can be searched.
        let lookups: Vec<(String, haven_addons::Catalog)> = self
            .installed
            .borrow()
            .iter()
            .flat_map(|addon| {
                addon
                    .manifest
                    .catalogs
                    .iter()
                    .filter(|catalog| catalog.kind == kind && catalog.searchable())
                    .map(|catalog| (addon.url.clone(), catalog.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut pending = 0;

        for server in self.servers.borrow().iter().cloned() {
            // The server the title was opened from holds it for certain;
            // the others are asked whether they have the same title.
            let own = origin
                .as_ref()
                .filter(|(from, _)| from.id == server.id && !renamed)
                .map(|(_, item)| item.clone());
            let offline = self.server_states.borrow().get(&server.id) == Some(&2);
            if own.is_none() && (!can_match || offline) {
                continue;
            }
            pending += 1;
            let (client, mut wanted) = (self.server_client.clone(), wanted.clone());
            let (addons, lookups) = (self.addons.clone(), lookups.clone());
            let current = self.stream_generation.shared();
            self.tasks.spawn(async move {
                let source = format!("{} ({})", server.name, server.kind.label());
                let found = async {
                    let item = match own {
                        Some(item) => Some(item),
                        None => {
                            if wanted.imdb.is_none() {
                                wanted.imdb = imdb_by_name(&addons, &lookups, &wanted).await;
                            }
                            client.find(&server, &wanted).await?
                        }
                    };
                    let Some(item) = item else { return Ok(None) };
                    let id = title_id(server.id, &item);
                    // A long list shows as it arrives, a few at a time.
                    client
                        .streams_live(&server, &item, |some| {
                            let (source, id) = (source.clone(), id.clone());
                            on_ui(move |library| {
                                library.streams_arrived(generation, &source, Some(id), &some)
                            });
                            // Stop reading once another title is open.
                            current.load(std::sync::atomic::Ordering::Relaxed) == generation
                        })
                        .await?;
                    Ok(Some(id))
                }
                .await
                .map_err(|err: haven_servers::Error| err.to_string());
                on_ui(move |library| match found {
                    Ok(item) => library.streams_loaded(generation, &source, item, Ok(Vec::new())),
                    Err(err) => library.streams_loaded(generation, &source, None, Err(err)),
                });
            });
        }

        // What add-ons know the film or episode as: its own id if it came
        // from an add-on, else the IMDb form (`tt…`, `tt…:season:episode`).
        let foreign = split_id(video_id).is_some() || video_id.starts_with("tmdb:");
        let addon_id = if !foreign {
            Some(video_id.to_owned())
        } else {
            match (&wanted.imdb, wanted.episode) {
                (Some(imdb), Some((season, episode))) if wanted.series => {
                    Some(format!("{imdb}:{season}:{episode}"))
                }
                (Some(imdb), _) if !wanted.series => Some(imdb.clone()),
                _ => None,
            }
        };
        if let Some(addon_id) = addon_id {
            let sources: Vec<(String, String)> = self
                .installed
                .borrow()
                .iter()
                .filter(|addon| addon.manifest.provides("stream", kind, &addon_id))
                .map(|addon| (addon.url.clone(), addon.manifest.name.clone()))
                .collect();
            for (url, name) in sources {
                pending += 1;
                let (addons, kind, addon_id) = (self.addons.clone(), kind.to_owned(), addon_id.clone());
                self.tasks.spawn(async move {
                    let streams =
                        addons.streams(&url, &kind, &addon_id).await.map_err(|err| err.to_string());
                    on_ui(move |library| library.streams_loaded(generation, &name, None, streams));
                });
            }
        }

        // The films and series of the user's Xtream Codes accounts.
        pending += self.ask_accounts(generation, &wanted);

        self.streams_pending.set(pending);
        if pending == 0 {
            self.give_up_autoplay();
            return self.set_streams_state(
                Some(heading),
                false,
                if super::NO_ADDONS {
                    "Nothing you have added can play this. Add a media server or an Xtream Codes account in Settings."
                } else {
                    "Nothing you have added can play this. Add a media server or an Xtream Codes account in Settings, or install an add-on that offers streams."
                },
            );
        }
        self.set_streams_state(Some(heading), true, "");
    }

    /// Some places to watch have come in from a media server or add-on
    /// (which may send more). `server_item` is the item they play when
    /// they come from a media server.
    pub(super) fn streams_arrived(
        &self,
        generation: u64,
        source: &str,
        server_item: Option<String>,
        streams: &[Stream],
    ) {
        if generation != self.stream_generation.get() {
            return;
        }
        // Playable here with the player, or on a Chromecast.
        let can_play = self.player.borrow().is_some() || !self.cast_devices.borrow().is_empty();
        for stream in streams {
            let Some(mut row) = stream_row(source, stream) else { continue };
            // Its provider: the add-on a server got it from, if the
            // server says; else the server or add-on that answered.
            row.group = stream.provider.as_deref().unwrap_or(source).into();
            // Only plain web addresses reach the player.
            let address = stream
                .url
                .as_deref()
                .filter(|_| can_play && stream.kind() == Some(StreamKind::Direct))
                .and_then(web_address)
                .map(str::to_owned);
            row.playable = address.is_some();
            let source = StreamSource { address, server_item: server_item.clone() };
            // Your own servers stay above the add-ons, and each
            // provider's rows stay together under its heading.
            let servers = self.server_rows.get();
            let (from, to) =
                if server_item.is_some() { (0, servers) } else { (servers, self.streams.row_count()) };
            let at = (from..to)
                .rev()
                .find(|&at| self.streams.row_data(at).is_some_and(|known| known.group == row.group))
                .map_or(to, |last| last + 1);
            self.stream_sources.borrow_mut().insert(at, source);
            self.streams.insert(at, row);
            if server_item.is_some() {
                self.server_rows.set(servers + 1);
            }
        }
        self.mark_stream_rows();
        self.show_stream_groups();
        // The next episode plays from the same place as the last, as
        // soon as the row of the same name shows.
        self.autoplay_from(source, false);
    }

    /// Works out, for every row, its place under its provider's
    /// heading: which rows show (ten per provider, more on asking),
    /// which carries the heading and which the "Show more".
    fn mark_stream_rows(&self) {
        let mut counts: std::collections::HashMap<slint::SharedString, i32> = Default::default();
        for row in self.streams.iter() {
            *counts.entry(row.group).or_default() += 1;
        }
        let limits = self.stream_limits.borrow();
        let mut places: std::collections::HashMap<slint::SharedString, i32> = Default::default();
        for at in 0..self.streams.row_count() {
            let Some(mut row) = self.streams.row_data(at) else { continue };
            let place = places.entry(row.group.clone()).or_default();
            let count = counts.get(&row.group).copied().unwrap_or(0);
            let limit = limits.get(row.group.as_str()).copied().unwrap_or(STREAMS_SHOWN);
            let marked = StreamRow {
                group_at: *place,
                shown: *place < limit,
                heading: if *place == 0 { format!("{}  ·  {count}", row.group).into() } else { "".into() },
                more: if *place == limit - 1 && count > limit { count - limit } else { 0 },
                ..row.clone()
            };
            *place += 1;
            if marked != row {
                row = marked;
                self.streams.set_row_data(at, row);
            }
        }
    }

    /// Shows ten more of a provider's places to watch.
    pub(super) fn show_more_streams(&self, group: &str) {
        *self.stream_limits.borrow_mut().entry(group.to_owned()).or_insert(STREAMS_SHOWN) +=
            STREAMS_SHOWN;
        self.mark_stream_rows();
    }

    /// Plays the next episode from `source` if that is where the last
    /// one played from: the row of the same name, or, once the source
    /// has `finished` answering, its first playable row.
    fn autoplay_from(&self, source: &str, finished: bool) {
        let wanted = self.autoplay.borrow().clone();
        let Some((wanted_source, wanted_name)) = wanted.filter(|(wanted, _)| wanted == source) else {
            return;
        };
        let rows: Vec<(usize, StreamRow)> = self
            .streams
            .iter()
            .enumerate()
            .filter(|(_, row)| row.playable && row.source == wanted_source.as_str())
            .collect();
        let pick = rows
            .iter()
            .find(|(_, row)| row.name == wanted_name.as_str())
            .or_else(|| rows.first().filter(|_| finished))
            .map(|(at, _)| *at);
        if let Some(at) = pick {
            *self.autoplay.borrow_mut() = None;
            self.play_stream(at as i32);
        }
    }

    /// One media server or add-on has finished answering.
    pub(super) fn streams_loaded(
        &self,
        generation: u64,
        source: &str,
        server_item: Option<String>,
        streams: Result<Vec<Stream>, String>,
    ) {
        if generation != self.stream_generation.get() {
            return;
        }
        match streams {
            Ok(streams) => self.streams_arrived(generation, source, server_item, &streams),
            Err(err) => haven_core::diag::note(&format!("{source} gave no places to watch: {err}")),
        }
        self.autoplay_from(source, true);
        self.show_stream_groups();
        let pending = self.streams_pending.get().saturating_sub(1);
        self.streams_pending.set(pending);
        if pending == 0 {
            self.give_up_autoplay();
            let note = if self.streams.row_count() == 0 {
                if super::NO_ADDONS {
                    "None of your media servers or accounts has this."
                } else {
                    "None of your media servers, accounts or add-ons has this."
                }
            } else {
                ""
            };
            self.set_streams_state(None, false, note);
        }
    }

    /// Lists the providers of the places to watch, in the order they
    /// first appear, each with how many it gave.
    fn show_stream_groups(&self) {
        let mut groups: Vec<crate::StreamGroup> = Vec::new();
        for row in self.streams.iter() {
            match groups.iter_mut().find(|group| group.name == row.group) {
                Some(group) => group.count += 1,
                None => groups.push(crate::StreamGroup { name: row.group.clone(), count: 1 }),
            }
        }
        if let Some(ui) = self.ui.upgrade() {
            // A chosen provider that is gone falls back to all.
            if !groups.iter().any(|group| group.name == ui.get_stream_group()) {
                ui.set_stream_group("".into());
            }
            if self.streams.row_count() == 0 {
                self.stream_limits.borrow_mut().clear();
            }
            ui.set_stream_groups(slint::ModelRc::new(slint::VecModel::from(groups)));
        }
    }

    /// The next episode's own source did not turn up: leave the player,
    /// so the list of places to watch it shows instead.
    fn give_up_autoplay(&self) {
        if self.autoplay.borrow_mut().take().is_some() {
            if let Some(dismiss) = self.player_dismiss.borrow().as_ref() {
                dismiss();
            }
        }
    }

    /// The episode after the one whose places to watch are listed.
    fn next_video(&self) -> Option<Video> {
        let open = self.open.borrow();
        let meta = open.as_ref()?.meta.as_ref()?;
        let current = self.listed.borrow().as_ref()?.video_id.clone();
        // In the order they are watched; specials (season 0) are not in the run.
        let mut run: Vec<&Video> =
            meta.videos.iter().filter(|video| video.season.is_none_or(|season| season > 0)).collect();
        run.sort_by_key(|video| (video.season, video.episode));
        let at = run.iter().position(|video| video.id == current)?;
        run.get(at + 1).map(|video| (*video).clone())
    }

    fn episode_label(video: &Video) -> String {
        let number = match (video.season, video.episode) {
            (Some(season), Some(episode)) => format!("S{season} E{episode}"),
            (None, Some(episode)) => format!("E{episode}"),
            _ => String::new(),
        };
        [number.as_str(), video.title.as_deref().unwrap_or_default()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// The player wants the next episode. Notes what was watched, opens
    /// the next episode on the title's page and plays it from the same
    /// place as the last as soon as that place answers. False if there
    /// is no next episode.
    #[cfg_attr(not(feature = "player"), allow(dead_code))]
    pub fn play_next(&self, played: Option<Played>) -> bool {
        if let Some(played) = played {
            self.report_played(&played);
            if played.finished {
                self.track_watched(&played.kind, &played.title_id, &played.video_id);
            }
        }
        self.reload_continue();
        let (Some(next), Some(last)) = (self.next_video(), self.last_played.borrow().clone()) else {
            return false;
        };
        if let Some(season) = next.season {
            if self.ui.upgrade().is_some_and(|ui| ui.get_season() != season as i32) {
                self.show_season(season as i32);
            }
        }
        *self.autoplay.borrow_mut() = Some(last);
        self.open_episode(&next.id);
        // Nothing to ask: the list is already showing instead.
        self.autoplay.borrow().is_some() || self.streams_pending.get() > 0
    }

    /// Plays the place to watch in that row of the list, if Haven can.
    /// What playing row `row` of the list asks for, and the media server
    /// item behind it, if any.
    pub(super) fn request_for(&self, row: i32) -> Option<(PlayRequest, Option<String>)> {
        let (address, server_item) = self
            .stream_sources
            .borrow()
            .get(row.max(0) as usize)
            .and_then(|source| Some((source.address.clone()?, source.server_item.clone())))?;
        let ui = self.ui.upgrade()?;
        let row_shown = self.streams.row_data(row.max(0) as usize);
        let (kind, title_id, poster) = self.open.borrow().as_ref().map(|open| {
            (
                open.kind.clone(),
                open.id.clone(),
                open.meta.as_ref().and_then(|meta| meta.poster.clone()),
            )
        })?;
        let (video_id, detail) = self
            .listed
            .borrow()
            .as_ref()
            .map(|listed| (listed.video_id.clone(), listed.detail.clone()))?;
        Some((
            PlayRequest {
                address,
                title: ui.get_detail().title.to_string(),
                detail,
                kind,
                title_id,
                video_id,
                poster,
                remember: true,
                source: row_shown.as_ref().map(|row| row.source.to_string()).unwrap_or_default(),
                next: self.next_video().map(|video| Self::episode_label(&video)),
            },
            server_item,
        ))
    }

    /// The season and episode numbers of the episode listed, if it is one.
    pub(super) fn listed_numbers(&self) -> (Option<u32>, Option<u32>) {
        let video_id = match self.listed.borrow().as_ref() {
            Some(listed) => listed.video_id.clone(),
            None => return (None, None),
        };
        let open = self.open.borrow();
        let video = open
            .as_ref()
            .and_then(|open| open.meta.as_ref())
            .and_then(|meta| meta.videos.iter().find(|video| video.id == video_id));
        match video {
            Some(video) if video.season.is_some() => (video.season, video.episode),
            _ => (None, None),
        }
    }

    pub(super) fn play_stream(&self, row: i32) {
        let Some((request, server_item)) = self.request_for(row) else { return };
        let row_shown = self.streams.row_data(row.max(0) as usize);
        *self.last_played.borrow_mut() =
            row_shown.map(|row| (row.source.to_string(), row.name.to_string()));
        if let Some(device) = self.cast_target() {
            return self.cast_to(device, request);
        }
        if let Some(play) = self.player.borrow().as_ref() {
            *self.playing_item.borrow_mut() = server_item;
            play(request);
        }
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
    // The detail goes on two lines: the release's name, then its facts.
    // The file's size is taken out of it and shown on its own, so it is
    // never cut off.
    let mut detail = one_line(stream.details().unwrap_or_default());
    let mut size = stream.video_size().map(size_text);
    if let Some((text, start, end)) = size_in(&detail) {
        size.get_or_insert(text);
        detail = tidy(&format!("{}{}", &detail[..start], &detail[end..]));
    }
    let parts: Vec<&str> = detail.split(" · ").collect();
    let mut first = String::new();
    let mut used = 0;
    for part in &parts {
        if used > 0 && first.chars().count() + part.chars().count() > 46 {
            break;
        }
        if used > 0 {
            first.push_str(" · ");
        }
        first.push_str(part);
        used += 1;
    }
    let rest = parts[used..].join(" · ");
    Some(StreamRow {
        source: source.into(),
        name: one_line(stream.name.as_deref().unwrap_or("Stream")).into(),
        detail: first.into(),
        detail2: rest.into(),
        size: size.unwrap_or_default().into(),
        kind: kind.into(),
        playable: false,
        group: source.into(),
        group_at: 0,
        shown: true,
        heading: "".into(),
        more: 0,
    })
}

/// Looks a title's IMDb id up by its name and year in the catalogues
/// that can be searched (the built-in free one, for a start).
async fn imdb_by_name(
    addons: &haven_addons::Client,
    lookups: &[(String, haven_addons::Catalog)],
    wanted: &Wanted,
) -> Option<String> {
    let plain = |name: &str| {
        name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>()
    };
    let year = |text: &Option<String>| {
        text.as_deref().and_then(|text| text.get(..4)).and_then(|year| year.parse::<u32>().ok())
    };
    let name = plain(&wanted.name);
    if name.is_empty() {
        return None;
    }
    for (url, catalog) in lookups {
        let Ok(found) = addons.catalog(url, catalog, Some(wanted.name.trim())).await else { continue };
        let hit = found.iter().find(|title| {
            is_imdb_id(&title.id)
                && plain(&title.name) == name
                && match (year(&wanted.year), year(&title.release_info)) {
                    (Some(a), Some(b)) => a == b,
                    _ => true,
                }
        });
        if let Some(hit) = hit {
            return Some(hit.id.clone());
        }
    }
    None
}

/// A size in bytes as people read it: "12.3 GB", "850 MB".
fn size_text(bytes: u64) -> String {
    let gb = bytes as f64 / 1_073_741_824.0;
    if gb >= 1.0 { format!("{gb:.1} GB") } else { format!("{:.0} MB", bytes as f64 / 1_048_576.0) }
}

/// The first size written in a text ("12.3 GB", "850MB", "1,4 GiB"), as
/// Haven shows it, with where it starts and ends (a 💾 before it counts
/// as part of it).
fn size_in(text: &str) -> Option<(String, usize, usize)> {
    const UNITS: [(&str, &str); 6] = [("TIB", "TB"), ("TB", "TB"), ("GIB", "GB"), ("GB", "GB"), ("MIB", "MB"), ("MB", "MB")];
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        let starts_number = bytes[at].is_ascii_digit() && (at == 0 || !bytes[at - 1].is_ascii_alphanumeric());
        if !starts_number {
            at += 1;
            continue;
        }
        let mut end = at;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || ((bytes[end] == b'.' || bytes[end] == b',') && end + 1 < bytes.len() && bytes[end + 1].is_ascii_digit())) {
            end += 1;
        }
        let number = text[at..end].replace(',', ".");
        let mut unit_at = end;
        while unit_at < bytes.len() && bytes[unit_at] == b' ' {
            unit_at += 1;
        }
        let rest = text[unit_at..].to_ascii_uppercase();
        let unit = UNITS.iter().find(|(written, _)| {
            rest.starts_with(written) && !rest.as_bytes().get(written.len()).is_some_and(|b| b.is_ascii_alphabetic())
        });
        if let Some((written, shown)) = unit {
            let mut start = at;
            let before = text[..at].trim_end();
            if before.ends_with('💾') {
                start = before.len() - '💾'.len_utf8();
            }
            return Some((format!("{number} {shown}"), start, unit_at + written.len()));
        }
        at = end.max(at + 1);
    }
    None
}

/// Cleans up a detail line once something was taken out of it: no
/// doubled spaces or separators, none left at either end.
fn tidy(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let parts: Vec<&str> = joined.split(" · ").map(str::trim).filter(|part| !part.is_empty() && *part != "·").collect();
    parts.join(" · ").trim_matches(|c: char| c == '·' || c == '|' || c.is_whitespace()).to_owned()
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn sizes_are_found_in_the_details() {
        let (size, start, end) = size_in("Movie.2024.2160p 💾 12.3 GB 👤 50").unwrap();
        assert_eq!(size, "12.3 GB");
        assert_eq!(tidy(&format!("{}{}", &"Movie.2024.2160p 💾 12.3 GB 👤 50"[..start], &"Movie.2024.2160p 💾 12.3 GB 👤 50"[end..])), "Movie.2024.2160p 👤 50");
        assert_eq!(size_in("1080p · 850MB · WEB").unwrap().0, "850 MB");
        assert_eq!(size_in("x 1,4 GiB").unwrap().0, "1.4 GB");
        assert_eq!(size_in("H.264 5.1 Atmos 2160p"), None);
        assert_eq!(size_in("Show.S01E02.MBC"), None);
    }

    #[test]
    fn byte_counts_read_well() {
        assert_eq!(size_text(13_207_024_435), "12.3 GB");
        assert_eq!(size_text(891_289_600), "850 MB");
    }
}

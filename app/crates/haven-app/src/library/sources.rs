//! Where a film or episode can be watched: every version of it on every
//! media server that has it, and every stream the add-ons offer for it.
//! The user picks one from the list on the title's page.

use haven_addons::{Stream, StreamKind};
use haven_metadata::is_imdb_id;
use haven_player::source::web_address;
use haven_servers::{Wanted, split_id, title_id};
use slint::Model;

use super::{Library, Listed, PlayRequest, on_ui};
use crate::StreamRow;

/// The heading over the list.
pub(super) const HEADING: &str = "Where to watch";

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
        *self.listed.borrow_mut() =
            Some(Listed { video_id: video_id.to_owned(), detail: String::new() });
        self.show_resume_note();

        let wanted = self.wanted(kind, video_id);
        // A series can only be matched down to an episode.
        let can_match = !wanted.name.trim().is_empty() && (!wanted.series || wanted.episode.is_some());
        let origin = self.server_item(video_id);
        let mut pending = 0;

        for server in self.servers.borrow().iter().cloned() {
            // The server the title was opened from holds it for certain;
            // the others are asked whether they have the same title.
            let own = origin
                .as_ref()
                .filter(|(from, _)| from.id == server.id)
                .map(|(_, item)| item.clone());
            let offline = self.server_states.borrow().get(&server.id) == Some(&2);
            if own.is_none() && (!can_match || offline) {
                continue;
            }
            pending += 1;
            let (client, wanted) = (self.server_client.clone(), wanted.clone());
            self.tasks.spawn(async move {
                let found = async {
                    let item = match own {
                        Some(item) => Some(item),
                        None => client.find(&server, &wanted).await?,
                    };
                    match item {
                        Some(item) => {
                            let streams = client.streams(&server, &item).await?;
                            Ok((Some(title_id(server.id, &item)), streams))
                        }
                        None => Ok((None, Vec::new())),
                    }
                }
                .await
                .map_err(|err: haven_servers::Error| err.to_string());
                let source = format!("{} ({})", server.name, server.kind.label());
                on_ui(move |library| match found {
                    Ok((item, streams)) => library.streams_loaded(generation, &source, item, Ok(streams)),
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

        self.streams_pending.set(pending);
        if pending == 0 {
            return self.set_streams_state(
                Some(heading),
                false,
                "Nothing you have added can play this. Add a media server in Settings, or install an add-on that offers streams.",
            );
        }
        self.set_streams_state(Some(heading), true, "");
    }

    /// One media server or add-on has answered. `server_item` is the
    /// item the streams play when they come from a media server.
    fn streams_loaded(
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
            Ok(streams) => {
                // Playable here with the player, or on a Chromecast.
                let can_play =
                    self.player.borrow().is_some() || !self.cast_devices.borrow().is_empty();
                for stream in &streams {
                    let Some(mut row) = stream_row(source, stream) else { continue };
                    // Only plain web addresses reach the player.
                    let address = stream
                        .url
                        .as_deref()
                        .filter(|_| can_play && stream.kind() == Some(StreamKind::Direct))
                        .and_then(web_address)
                        .map(str::to_owned);
                    row.playable = address.is_some();
                    let source = StreamSource { address, server_item: server_item.clone() };
                    if server_item.is_some() {
                        // Your own servers stay above the add-ons.
                        let at = self.server_rows.get();
                        self.stream_sources.borrow_mut().insert(at, source);
                        self.streams.insert(at, row);
                        self.server_rows.set(at + 1);
                    } else {
                        self.stream_sources.borrow_mut().push(source);
                        self.streams.push(row);
                    }
                }
            }
            Err(err) => eprintln!("haven: {source} gave no streams: {err}"),
        }
        let pending = self.streams_pending.get().saturating_sub(1);
        self.streams_pending.set(pending);
        if pending == 0 {
            let note = if self.streams.row_count() == 0 {
                "None of your media servers or add-ons has this."
            } else {
                ""
            };
            self.set_streams_state(None, false, note);
        }
    }

    /// Plays the place to watch in that row of the list, if Haven can.
    pub(super) fn play_stream(&self, row: i32) {
        let Some((address, server_item)) = self
            .stream_sources
            .borrow()
            .get(row.max(0) as usize)
            .and_then(|source| Some((source.address.clone()?, source.server_item.clone())))
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
    Some(StreamRow {
        source: source.into(),
        name: one_line(stream.name.as_deref().unwrap_or("Stream")).into(),
        detail: one_line(stream.details().unwrap_or_default()).into(),
        kind: kind.into(),
        playable: false,
    })
}

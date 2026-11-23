//! Debug-build helpers for checking the app without a person at the
//! screen. All are driven by environment variables and do nothing unless
//! set:
//!
//! - `HAVEN_DATA_DIR=<folder>`: keep the database there (read in `main`).
//! - `HAVEN_PAGE=<index>`: open on that page without saving the choice.
//! - `HAVEN_INSTALL=<address>`: install that add-on at start.
//! - `HAVEN_WINDOW=<width>x<height>`: window size in physical pixels.
//! - `HAVEN_SEARCH=<words>`: run that search at start.
//! - `HAVEN_OPEN=<type> <id>`: open that title's page at start.
//! - `HAVEN_EPISODE=<video id>`: pick that episode after
//!   `HAVEN_EPISODE_AT` seconds (default 4).
//! - `HAVEN_DO=<seconds>:<action>;…`: a script, each action at its time.
//!   Actions: `play <row>` (a row of the stream list), `pause`,
//!   `seek <seconds>`, `skip <seconds>`, `audio <id>`, `sub <id>` (0 off),
//!   `fullscreen`, `menu <0|1|2>` (none, audio, subtitles), `close`, `home` (close the title page, go to Home),
//!   `continue <card>` (open a Continue Watching card), `watchlist`
//!   (add or remove the open title), `page <index>`, `addtv <address>`,
//!   `channel <n>`, `station <n>`, `tvfilter <words>`, `tvgroup <name>`,
//!   `tvstate`, `signin <jellyfin|emby> <address> <user> [password]`,
//!   `open <type> <id>`, `servers`, `connect <0|1>` (Trakt, Simkl),
//!   `tracking`, `findcast`, `cast` (print the casting state),
//!   `checkservers`, `xtream <server> <user> <password>`,
//!   `explore <source> <kind> <genre>` (genre -1 for all), `more`,
//!   `explorestate`, `shelves` (print Home's shelves), `sources` (print
//!   the places to watch), `librarytab <n>`, `section <n>`, `band <n>`,
//!   `week <0|1>`, `download <row>`, `snap <file.png>`, `wheel <x> <y> <dy>`, `settingsy <y>`, `state` (print the
//!   player's state).
//! - `HAVEN_TMDB_API=<address>` with `HAVEN_TMDB_IMAGES=<address>`: ask a
//!   stand-in for TMDB (read in `library.rs`).
//! - `HAVEN_PUMP=<seconds>`: draw the window that often even when it is
//!   hidden behind others (macOS does not draw a covered window, and the
//!   player only starts once the window draws).
//! - `HAVEN_SNAPSHOT=<file.png>`: save a picture of the window after
//!   `HAVEN_SNAPSHOT_AT` seconds (default 5).
//! - `HAVEN_QUIT_AFTER=<seconds>`: close by itself.

use std::{error::Error, time::Duration};

use slint::{ComponentHandle, Timer, TimerMode};

use slint::Model;

use crate::{AppWindow, Poster};

fn seconds(name: &str) -> Option<Duration> {
    std::env::var(name).ok()?.parse::<f64>().ok().map(Duration::from_secs_f64)
}

fn save_png(path: &str, width: u32, height: u32, rgba: &[u8]) -> Result<(), Box<dyn Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

fn snap(ui: &AppWindow, path: &str) {
    let saved = ui
        .window()
        .take_snapshot()
        .map_err(Into::into)
        .and_then(|shot| save_png(path, shot.width(), shot.height(), shot.as_bytes()));
    match saved {
        Ok(()) => eprintln!("haven: saved a window snapshot to {path}"),
        Err(err) => eprintln!("haven: window snapshot failed: {err}"),
    }
}

/// One step of a `HAVEN_DO` script.
fn act(ui: &AppWindow, action: &str) {
    let (verb, rest) = action.split_once(' ').unwrap_or((action, ""));
    let number = rest.trim().parse::<f32>().unwrap_or(0.0);
    match verb {
        "play" => ui.invoke_play_stream(number as i32),
        "pause" => ui.invoke_player_toggle_pause(),
        "seek" => ui.invoke_player_seek(number),
        "skip" => ui.invoke_player_skip(number),
        "audio" => ui.invoke_player_choose_audio(number as i32),
        "sub" => ui.invoke_player_choose_subtitle(number as i32),
        "fullscreen" => ui.invoke_player_toggle_fullscreen(),
        "menu" => ui.set_player_menu(number as i32),
        "close" => ui.invoke_player_close(),
        "home" => {
            ui.invoke_close_detail();
            ui.set_page(0);
        }
        "continue" => {
            if let Some(card) = ui.get_continue_shelf().items.row_data(number as usize) {
                ui.invoke_open_title(card);
            }
        }
        "watchlist" => ui.invoke_toggle_watchlist(),
        "page" => {
            ui.invoke_close_detail();
            ui.set_page(number as i32);
        }
        "addtv" => ui.invoke_add_tv_source(rest.trim().into()),
        "removetv" => ui.invoke_remove_tv_source(rest.trim().into()),
        "channel" => ui.invoke_play_channel(false, number as i32),
        "station" => ui.invoke_play_channel(true, number as i32),
        "tvfilter" => {
            ui.set_tv_words(rest.trim().into());
            ui.invoke_filter_channels(false, rest.trim().into(), ui.get_tv_category());
        }
        // tvgroup <name>: a group of the playlist; favgroup <name>: one of
        // the user's Favourites groups; tvgroup with nothing: every channel.
        "tvgroup" | "favgroup" => {
            let key = match (verb, rest.trim()) {
                (_, "") => String::new(),
                ("favgroup", name) => format!("f:{name}"),
                (_, name) => format!("g:{name}"),
            };
            ui.set_tv_category(key.as_str().into());
            ui.invoke_filter_channels(false, ui.get_tv_words(), key.into());
        }
        "catfilter" => ui.invoke_filter_categories(false, rest.trim().into()),
        "star" => ui.invoke_fav_pick(false, number as i32),
        "favtoggle" => ui.invoke_fav_toggle(rest.trim().into()),
        "favnew" => ui.invoke_fav_new_group(rest.trim().into()),
        "favdelete" => ui.invoke_fav_delete_group(rest.trim().into()),
        "favclose" => ui.invoke_fav_close(),
        "tvsource" => {
            // tvsource <on|off> <row>
            let mut parts = rest.split_whitespace();
            let on = parts.next() == Some("on");
            let row = parts.next().and_then(|row| row.parse::<usize>().ok()).unwrap_or(0);
            if let Some(source) = ui.get_tv_sources().row_data(row) {
                ui.invoke_set_tv_source_enabled(source.url, on);
            }
        }
        #[cfg(feature = "addons")]
        "addon" => {
            // addon <on|off> <row>
            let mut parts = rest.split_whitespace();
            let on = parts.next() == Some("on");
            let row = parts.next().and_then(|row| row.parse::<usize>().ok()).unwrap_or(0);
            if let Some(addon) = ui.get_addons().row_data(row) {
                ui.invoke_set_addon_enabled(addon.url, on);
            }
        }
        "server" => {
            // server <on|off> <row>
            let mut parts = rest.split_whitespace();
            let on = parts.next() == Some("on");
            let row = parts.next().and_then(|row| row.parse::<usize>().ok()).unwrap_or(0);
            if let Some(server) = ui.get_servers().row_data(row) {
                ui.invoke_set_server_enabled(server.id, on);
            }
        }
        #[cfg(feature = "addons")]
        "installshared" => ui.invoke_install_shared(number as i32),
        #[cfg(feature = "addons")]
        "addons" => eprintln!(
            "haven-addons: status=\"{}\" installed=[{}] shared=[{}]",
            ui.get_addon_status(),
            ui.get_addons()
                .iter()
                .map(|addon| format!("{} {}", addon.name, if addon.enabled { "on" } else { "off" }))
                .collect::<Vec<_>>()
                .join(" | "),
            ui.get_shared_addons()
                .iter()
                .map(|addon| format!("{} on {}{}", addon.name, addon.server, if addon.installed { " (installed)" } else { "" }))
                .collect::<Vec<_>>()
                .join(" | "),
        ),
        "offers" => eprintln!(
            "haven-offers: note=\"{}\" link=\"{}\" rows=[{}]",
            ui.get_offers_note(),
            ui.get_offers_link(),
            ui.get_offers()
                .iter()
                .map(|offer| format!("{}: {}", offer.name, offer.ways))
                .collect::<Vec<_>>()
                .join(" | "),
        ),
        "country" => {
            ui.set_country_index(number as i32);
            ui.invoke_country_selected(number as i32);
        }
        "tvstate" => eprintln!(
            "haven-tv: status=\"{}\" xtream=\"{}\" sources=[{}] tv={}/{} radio={}/{} category=\"{}\" \
             categories=[{}] fav-open={} picks=[{}] first=[{}]",
            ui.get_tv_status(),
            ui.get_xtream_status(),
            ui.get_tv_sources()
                .iter()
                .map(|source| format!(
                    "{}: {} ({}) {} {}",
                    source.name,
                    source.detail,
                    source.channels,
                    if source.enabled { "on" } else { "off" },
                    ["", "checking", "online", "offline"][source.state.clamp(0, 3) as usize]
                ))
                .collect::<Vec<_>>()
                .join(" | "),
            ui.get_tv_channels().row_count(),
            ui.get_tv_total(),
            ui.get_radio_channels().row_count(),
            ui.get_radio_total(),
            ui.get_tv_category(),
            ui.get_tv_categories()
                .iter()
                .map(|row| format!("{}{}:{}", if row.favourite { "★" } else { "" }, row.label, row.count))
                .collect::<Vec<_>>()
                .join("|"),
            ui.get_fav_open(),
            ui.get_fav_picks()
                .iter()
                .map(|pick| format!("{}{}", pick.name, if pick.has { "✓" } else { "" }))
                .collect::<Vec<_>>()
                .join("|"),
            ui.get_tv_channels()
                .iter()
                .take(3)
                .map(|row| format!("{}{}", if row.starred { "★" } else { "" }, row.name))
                .collect::<Vec<_>>()
                .join("|"),
        ),
        "signin" => {
            // signin <jellyfin|emby> <address> <user> [password]
            let mut parts = rest.split_whitespace();
            let kind = match parts.next() {
                Some("emby") => 1,
                _ => 0,
            };
            let address = parts.next().unwrap_or_default();
            let user = parts.next().unwrap_or_default();
            let password = parts.next().unwrap_or_default();
            ui.invoke_server_sign_in(kind, address.into(), user.into(), password.into());
        }
        "showsources" => ui.invoke_show_sources(),
        "backupnow" => ui.invoke_backup_now(),
        "whatsnew" => eprintln!("haven-whatsnew: shown={} version=\"{}\" sections={}", ui.get_whats_new_shown(), ui.get_whats_new_version(), ui.get_whats_new_sections().row_count()),
        "whatsnewdone" => ui.invoke_whats_new_done(true),
        "local" => eprintln!(
            "haven-local: status=\"{}\" rows=[{}]",
            ui.get_local_status(),
            ui.get_local_rows().iter().take(5).map(|r| format!("{} ({})", r.title, r.detail)).collect::<Vec<_>>().join(" | ")
        ),
        "localopen" => ui.invoke_local_open(),
        "localplay" => ui.invoke_local_play(number as i32),
        "restore" => ui.invoke_backup_restore(),
        "backupstatus" => eprintln!("haven-backup: busy={} where=\"{}\" status=\"{}\"", ui.get_backup_busy(), ui.get_backup_where(), ui.get_backup_status()),
        "open" => {
            if let Some((kind, id)) = rest.trim().split_once(' ') {
                ui.invoke_open_title(Poster { id: id.into(), kind: kind.into(), ..Poster::default() });
            }
        }
        "servers" => {
            let shelves = ui.get_server_shelves();
            eprintln!(
                "haven-servers: status=\"{}\" servers=[{}] shelves=[{}]",
                ui.get_server_status(),
                ui.get_servers()
                    .iter()
                    .map(|server| format!(
                        "{} {} {}",
                        server.name,
                        server.kind,
                        ["checking", "online", "offline"][server.state.clamp(0, 2) as usize]
                    ))
                    .collect::<Vec<_>>()
                    .join(" | "),
                shelves
                    .iter()
                    .map(|shelf| format!(
                        "{}:{}{}",
                        shelf.title,
                        shelf.items.row_count(),
                        shelf.items.row_data(0).map(|p| format!(" first={} {}", p.kind, p.id)).unwrap_or_default()
                    ))
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
        }
        "checkservers" => ui.invoke_check_servers(),
        // click <x> <y> / rclick <x> <y>: a mouse click at that place, in
        // logical pixels from the window's top left.
        "click" | "rclick" => {
            let mut parts = rest.split_whitespace().filter_map(|part| part.parse::<f32>().ok());
            if let (Some(x), Some(y)) = (parts.next(), parts.next()) {
                use slint::platform::{PointerEventButton, WindowEvent};
                let position = slint::LogicalPosition::new(x, y);
                let button =
                    if verb == "click" { PointerEventButton::Left } else { PointerEventButton::Right };
                let window = ui.window();
                window.dispatch_event(WindowEvent::PointerMoved { position });
                window.dispatch_event(WindowEvent::PointerPressed { position, button });
                window.dispatch_event(WindowEvent::PointerReleased { position, button });
            }
        }
        // wheel <x> <y> <dy>: turns the mouse wheel there (negative dy
        // scrolls down), in logical pixels.
        "wheel" => {
            let mut parts = rest.split_whitespace().filter_map(|part| part.parse::<f32>().ok());
            if let (Some(x), Some(y), Some(delta_y)) = (parts.next(), parts.next(), parts.next()) {
                use slint::platform::WindowEvent;
                let position = slint::LogicalPosition::new(x, y);
                let window = ui.window();
                window.dispatch_event(WindowEvent::PointerMoved { position });
                window.dispatch_event(WindowEvent::PointerScrolled { position, delta_x: 0.0, delta_y });
            }
        }
        // settingsy <y>: scrolls the Settings page down that far.
        "settingsy" => ui.set_settings_scroll(-number),
        // librarytab <0|1|2|3> (Local, Downloads, Watchlist, History), section <0..6> (a part of Settings),
        // band <0|1> (Live TV or Radio), week <0|1> (the calendar).
        "librarytab" => ui.set_library_tab(number as i32),
        "section" => {
            ui.set_settings_section(number as i32);
            ui.set_settings_picking(false);
        }
        "band" => ui.set_live_band(number as i32),
        "week" => ui.invoke_calendar_week(number > 0.0),
        "download" => ui.invoke_download_stream(number as i32),
        "downloads" => eprintln!(
            "haven-downloads: [{}]",
            ui.get_download_rows()
                .iter()
                .map(|row| format!("{}|{}|{}|{}|{}", row.section, row.group, row.title, row.status, row.done))
                .collect::<Vec<_>>()
                .join(" ; ")
        ),
        "undownload" => ui.invoke_download_remove(number as i32),
        "streamgroup" => ui.set_stream_group(rest.trim().into()),
        "volume" => ui.invoke_player_set_volume(number),
        "mute" => ui.invoke_player_toggle_mute(),
        "passthrough" => ui.invoke_player_set_passthrough(number > 0.0),
        "speed" => ui.invoke_player_set_speed(number),
        "fit" => ui.invoke_player_set_fit(number as i32),
        "subsize" => ui.invoke_player_subtitle_scale_step(number as i32),
        "subdelay" => ui.invoke_player_subtitle_delay_step(number as i32),
        "subsync" => ui.invoke_player_subtitle_sync(number as i32),
        "next" => ui.invoke_player_next(),
        "cancelnext" => ui.invoke_player_cancel_next(),
        "controls" => eprintln!(
            "haven-controls: open={} title=\"{}\" sub=\"{}\" elapsed={} left={} ends=\"{}\" volume={:.2} muted={} \
             speed={} fit={} subsize={:.1} subdelay={:.1} next=\"{}\" countdown={} busy={} error=\"{}\" info=\"{}\"",
            ui.get_player_open(),
            ui.get_player_title(),
            ui.get_player_subtitle(),
            ui.get_player_elapsed(),
            ui.get_player_remaining(),
            ui.get_player_ends_at(),
            ui.get_player_volume(),
            ui.get_player_muted(),
            ui.get_player_speed(),
            ui.get_player_fit(),
            ui.get_player_subtitle_scale(),
            ui.get_player_subtitle_delay(),
            ui.get_player_next_title(),
            ui.get_player_next_countdown(),
            ui.get_player_busy(),
            ui.get_player_error(),
            ui.get_player_info().replace('\n', " / "),
        ),
        "clearcache" => ui.invoke_clear_cache(),
        "installupdate" => ui.invoke_install_update(),
        "playerchoice" => {
            let (kind, program) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
            ui.invoke_set_player_program(program.into());
            ui.invoke_set_player_choice(kind.parse().unwrap_or(0));
        }
        "playnote" => eprintln!("haven-playnote: \"{}\" player-open={}", ui.get_play_note(), ui.get_player_open()),
        "updatestate" => eprintln!(
            "haven-update: version=\"{}\" busy={} status=\"{}\"",
            ui.get_update_version(),
            ui.get_update_busy(),
            ui.get_update_status()
        ),
        "uncontinue" => {
            if let Some(card) = ui.get_continue_shelf().items.row_data(number as usize) {
                ui.invoke_remove_continue(card);
            }
        }
        "cache" => eprintln!("haven-cache: {}", ui.get_cache_status()),
        "xtream" => {
            let mut parts = rest.split_whitespace();
            let server = parts.next().unwrap_or_default();
            let user = parts.next().unwrap_or_default();
            let password = parts.next().unwrap_or_default();
            ui.invoke_add_xtream(server.into(), user.into(), password.into());
        }
        "explore" => {
            let mut parts = rest.split_whitespace().map(|part| part.parse::<i32>().unwrap_or(0));
            let source = parts.next().unwrap_or(0);
            let kind = parts.next().unwrap_or(0);
            let genre = parts.next().unwrap_or(-1);
            ui.invoke_explore_pick(source, kind, genre);
        }
        "more" => ui.invoke_explore_load_more(),
        "explorestate" => {
            let names = |rows: slint::ModelRc<slint::SharedString>| {
                rows.iter().map(|name| name.to_string()).collect::<Vec<_>>().join("|")
            };
            let items = ui.get_explore_items();
            eprintln!(
                "haven-explore: sources=[{}] source={} kinds=[{}] kind={} genres=[{}] genre={} \
                 items={} pictures={} loading={} more={} note=\"{}\" first=[{}]",
                ui.get_explore_sources()
                    .iter()
                    .map(|row| if row.header { format!("# {}", row.label) } else { format!("{}:{}", row.index, row.label) })
                    .collect::<Vec<_>>()
                    .join("|"),
                ui.get_explore_source(),
                names(ui.get_explore_kinds()),
                ui.get_explore_kind(),
                names(ui.get_explore_genres()),
                ui.get_explore_genre(),
                items.row_count(),
                items.iter().filter(|poster| poster.has_image).count(),
                ui.get_explore_loading(),
                ui.get_explore_more(),
                ui.get_explore_note(),
                items
                    .iter()
                    .take(3)
                    .map(|poster| format!("{} {} {}", poster.kind, poster.id, poster.title))
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
        }
        "shelves" => {
            let list = |name: &str, shelves: slint::ModelRc<crate::Shelf>| {
                eprintln!(
                    "haven-home: {name}=[{}]",
                    shelves
                        .iter()
                        .map(|shelf| format!(
                            "{} ({}):{}/{}{}",
                            shelf.title,
                            shelf.source,
                            shelf.items.row_count(),
                            shelf.items.iter().filter(|poster| poster.has_image).count(),
                            if !shelf.error.is_empty() { format!(" ERROR {}", shelf.error) } else if shelf.loading { " loading".to_owned() } else { String::new() },
                        ))
                        .collect::<Vec<_>>()
                        .join(" | "),
                );
            };
            list("servers", ui.get_server_shelves());
            list("catalogues", ui.get_shelves());
            list("tmdb", ui.get_catalog_shelves());
            list("search", ui.get_search_shelves());
        }
        "sources" => {
            let detail = ui.get_detail();
            eprintln!(
                "haven-sources: title=\"{}\" facts=\"{}\" poster={} background={} error=\"{}\" \
                 heading=\"{}\" loading={} note=\"{}\" episodes={} groups=[{}] rows=[{}]",
                detail.title,
                detail.facts,
                detail.has_poster,
                detail.has_background,
                detail.error,
                ui.get_streams_title(),
                ui.get_streams_loading(),
                ui.get_streams_note(),
                ui.get_episodes().row_count(),
                ui.get_stream_groups()
                    .iter()
                    .map(|group| format!("{}:{}", group.name, group.count))
                    .collect::<Vec<_>>()
                    .join("|"),
                ui.get_streams()
                    .iter()
                    .map(|row| format!(
                        "{}{}{} · {} · {} · {}",
                        if row.playable { "▶ " } else { "" },
                        ["", "[downloading] ", "[saved] "][row.saved.clamp(0, 2) as usize],
                        row.name,
                        row.source,
                        [row.detail.as_str(), row.detail2.as_str()]
                            .into_iter()
                            .filter(|part| !part.is_empty())
                            .collect::<Vec<_>>()
                            .join(" · "),
                        row.kind
                    ))
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
        }
        "episode" => ui.invoke_episode_selected(rest.trim().into()),
        "episodes" => eprintln!(
            "haven-episodes: season={} [{}]",
            ui.get_season(),
            ui.get_episodes()
                .iter()
                .map(|row| format!("{} {} {}", row.id, row.number, row.title))
                .collect::<Vec<_>>()
                .join(" | "),
        ),
        "connect" => ui.invoke_tracking_connect(number as i32),
        "unlink" => ui.invoke_tracking_disconnect(number as i32),
        "tracking" => eprintln!(
            "haven-tracking: status=\"{}\" link=\"{}\" keys=\"{}\" rows=[{}]",
            ui.get_tracking_status(),
            ui.get_tracking_link(),
            ui.get_keys_status(),
            ui.get_tracking()
                .iter()
                .map(|row| format!("{} connected={} can={}", row.name, row.connected, row.can_connect))
                .collect::<Vec<_>>()
                .join(" | "),
        ),
        "findcast" => ui.invoke_find_cast_devices(),
        "cast" => eprintln!(
            "haven-cast: finding={} devices=[{}] note=\"{}\" casting={} status=\"{}\"",
            ui.get_cast_finding(),
            ui.get_cast_devices().iter().map(|d| d.to_string()).collect::<Vec<_>>().join(" | "),
            ui.get_cast_note(),
            ui.get_casting(),
            ui.get_cast_status(),
        ),
        "snap" => snap(ui, rest.trim()),
        "state" => {
            let tracks = |rows: slint::ModelRc<crate::TrackRow>| {
                rows.iter()
                    .map(|row| format!("{}{}:{}", if row.selected { "*" } else { "" }, row.id, row.label))
                    .collect::<Vec<_>>()
                    .join(" | ")
            };
            let streams = ui.get_streams();
            let continuing = ui.get_continue_shelf().items;
            eprintln!(
                "haven-state: open={} busy={} paused={} pos={:.1} dur={:.1} time=\"{}\" error=\"{}\" \
                 title=\"{}\" sub=\"{}\" audio=[{}] subs=[{}] streams={} playable={} resume=\"{}\" \
                 continue=[{}]",
                ui.get_player_open(),
                ui.get_player_busy(),
                ui.get_player_paused(),
                ui.get_player_position(),
                ui.get_player_duration(),
                ui.get_player_time(),
                ui.get_player_error(),
                ui.get_player_title(),
                ui.get_player_subtitle(),
                tracks(ui.get_player_audio()),
                tracks(ui.get_player_subtitles()),
                streams.row_count(),
                streams.iter().filter(|row| row.playable).count(),
                ui.get_resume_note(),
                continuing
                    .iter()
                    .map(|card| format!("{} / {} / {:.2}", card.title, card.year, card.progress))
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
        }
        other => eprintln!("haven: HAVEN_DO does not know \"{other}\""),
    }
}

/// Returns the timers; keep them alive for as long as the window runs.
pub fn start(ui: &AppWindow) -> Vec<Timer> {
    let mut timers = Vec::new();
    if let Some(page) = std::env::var("HAVEN_PAGE").ok().and_then(|page| page.parse().ok()) {
        ui.set_page(page);
    }
    #[cfg(feature = "addons")]
    if let Ok(address) = std::env::var("HAVEN_INSTALL") {
        ui.invoke_install_addon(address.into());
    }
    if let Some((width, height)) = std::env::var("HAVEN_WINDOW")
        .ok()
        .and_then(|size| size.split_once('x').map(|(w, h)| (w.parse().ok(), h.parse().ok())))
        .and_then(|(w, h)| w.zip(h))
    {
        ui.window().set_size(slint::PhysicalSize::new(width, height));
    }
    if let Ok(words) = std::env::var("HAVEN_SEARCH") {
        ui.set_search_query(words.as_str().into());
        ui.invoke_search(words.into());
    }
    if let Some((kind, id)) = std::env::var("HAVEN_OPEN").ok().and_then(|title| {
        title.split_once(' ').map(|(kind, id)| (kind.to_owned(), id.to_owned()))
    }) {
        ui.invoke_open_title(Poster {
            id: id.into(),
            kind: kind.into(),
            ..Poster::default()
        });
    }
    if let Ok(video) = std::env::var("HAVEN_EPISODE") {
        let ui = ui.as_weak();
        let timer = Timer::default();
        let at = seconds("HAVEN_EPISODE_AT").unwrap_or(Duration::from_secs(4));
        timer.start(TimerMode::SingleShot, at, move || {
            if let Some(ui) = ui.upgrade() {
                ui.invoke_episode_selected(video.as_str().into());
            }
        });
        timers.push(timer);
    }
    if let Ok(script) = std::env::var("HAVEN_DO") {
        for step in script.split(';').filter(|step| !step.trim().is_empty()) {
            let Some((at, action)) = step.trim().split_once(':') else {
                eprintln!("haven: HAVEN_DO step without a time: {step}");
                continue;
            };
            let Ok(at) = at.trim().parse::<f64>() else {
                eprintln!("haven: HAVEN_DO step with a bad time: {step}");
                continue;
            };
            let (ui, action) = (ui.as_weak(), action.trim().to_owned());
            let timer = Timer::default();
            timer.start(TimerMode::SingleShot, Duration::from_secs_f64(at), move || {
                if let Some(ui) = ui.upgrade() {
                    act(&ui, &action);
                }
            });
            timers.push(timer);
        }
    }
    if let Some(every) = seconds("HAVEN_PUMP") {
        let ui = ui.as_weak();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, every, move || {
            if let Some(ui) = ui.upgrade() {
                let started = std::time::Instant::now();
                let _ = ui.window().take_snapshot();
                if std::env::var_os("HAVEN_PUMP_TIMING").is_some() {
                    eprintln!("haven: pump drew in {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
                }
            }
        });
        timers.push(timer);
    }
    if let Ok(path) = std::env::var("HAVEN_SNAPSHOT") {
        let ui = ui.as_weak();
        let timer = Timer::default();
        let at = seconds("HAVEN_SNAPSHOT_AT").unwrap_or(Duration::from_secs(5));
        timer.start(TimerMode::SingleShot, at, move || {
            if let Some(ui) = ui.upgrade() {
                snap(&ui, &path);
            }
        });
        timers.push(timer);
    }
    if let Some(after) = seconds("HAVEN_QUIT_AFTER") {
        let timer = Timer::default();
        timer.start(TimerMode::SingleShot, after, || {
            let _ = slint::quit_event_loop();
        });
        timers.push(timer);
    }
    timers
}

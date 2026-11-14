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
//!   `snap <file.png>`, `state` (print the player's state).
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
        "channel" => ui.invoke_play_channel(false, number as i32),
        "station" => ui.invoke_play_channel(true, number as i32),
        "tvfilter" => {
            ui.set_tv_words(rest.trim().into());
            ui.invoke_filter_channels(false, rest.trim().into(), ui.get_tv_group());
        }
        "tvgroup" => {
            ui.set_tv_group(rest.trim().into());
            ui.invoke_filter_channels(false, ui.get_tv_words(), rest.trim().into());
        }
        "tvstate" => eprintln!(
            "haven-tv: status=\"{}\" sources={} tv={}/{} radio={}/{} groups={}",
            ui.get_tv_status(),
            ui.get_tv_sources().row_count(),
            ui.get_tv_channels().row_count(),
            ui.get_tv_total(),
            ui.get_radio_channels().row_count(),
            ui.get_radio_total(),
            ui.get_tv_groups().iter().map(|g| g.to_string()).collect::<Vec<_>>().join("|"),
        ),
        "signin" => {
            // signin <jellyfin|emby> <address> <user> [password]
            let mut parts = rest.split_whitespace();
            let kind = if parts.next() == Some("emby") { 1 } else { 0 };
            let address = parts.next().unwrap_or_default();
            let user = parts.next().unwrap_or_default();
            let password = parts.next().unwrap_or_default();
            ui.invoke_server_sign_in(kind, address.into(), user.into(), password.into());
        }
        "open" => {
            if let Some((kind, id)) = rest.trim().split_once(' ') {
                ui.invoke_open_title(Poster { id: id.into(), kind: kind.into(), ..Poster::default() });
            }
        }
        "servers" => {
            let shelves = ui.get_server_shelves();
            eprintln!(
                "haven-servers: status=\"{}\" servers={} shelves=[{}]",
                ui.get_server_status(),
                ui.get_servers().row_count(),
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
        "connect" => ui.invoke_tracking_connect(number as i32),
        "tracking" => eprintln!(
            "haven-tracking: status=\"{}\" keys=\"{}\" rows=[{}]",
            ui.get_tracking_status(),
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

//! The player page: the video behind it, its controls, and remembering
//! how far each film or episode was watched.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use haven_core::storage::{Progress, Store};
use haven_player::{
    clock,
    resume::{self, Watched},
    tracks::TrackKind,
    video::{Event, Fit, Video},
};
use slint::{ComponentHandle, Image, ModelRc, Timer, TimerMode, VecModel};

use crate::{
    AppWindow, TrackRow,
    library::{PlayRequest, Played},
};

/// How often the place is saved while playing.
const SAVE_EVERY: Duration = Duration::from_secs(5);
/// After a seek the position on screen is left alone this long, so the
/// bar does not jump back while mpv catches up.
const SEEK_HOLD: Duration = Duration::from_millis(700);
/// Height of the bar of controls at the bottom of the player, plus a
/// little air, in logical pixels: subtitles are lifted clear of it.
const CONTROLS_HEIGHT: f32 = 112.0;
/// mpv's own subtitle margin, used while the controls are hidden.
const SUBTITLE_MARGIN: i64 = 22;
/// How long the next episode waits before it starts by itself.
const NEXT_AFTER: Duration = Duration::from_secs(10);
const PREFERENCES: &str = "player";

/// What the player keeps between runs.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Preferences {
    /// 0 silent to 1 as recorded.
    volume: f64,
    muted: bool,
    subtitle_scale: f64,
}

impl Default for Preferences {
    fn default() -> Self {
        Self { volume: 1.0, muted: false, subtitle_scale: 1.0 }
    }
}

struct Playing {
    request: PlayRequest,
    /// mpv has opened the stream.
    loaded: bool,
    failed: bool,
    position: f64,
    duration: f64,
    saved_at: Instant,
}

pub struct Player {
    ui: slint::Weak<AppWindow>,
    store: Rc<Store>,
    video: RefCell<Video>,
    playing: RefCell<Option<Playing>>,
    seek_held: Cell<Option<Instant>>,
    /// The bars of controls are on screen.
    controls_shown: Cell<bool>,
    /// The subtitle margin mpv was last given.
    margin_set: Cell<Option<i64>>,
    audio: Rc<VecModel<TrackRow>>,
    subtitles: Rc<VecModel<TrackRow>>,
    /// mpv's track list as last shown, to rebuild the menus only on change.
    tracks_shown: RefCell<String>,
    /// Why the player cannot work on this machine, once known.
    unusable: RefCell<Option<String>>,
    closed: RefCell<Option<Box<dyn Fn(Option<Played>)>>>,
    /// Starts the episode after the one that was playing; false if it cannot.
    next: RefCell<Option<Box<dyn Fn(Option<Played>) -> bool>>>,
    /// When the next episode starts by itself, while that is counting down.
    next_at: Cell<Option<Instant>>,
    preferences: Cell<Preferences>,
    speed: Cell<f64>,
    fit: Cell<i32>,
    subtitle_delay: Cell<f64>,
    ticker: Timer,
}

impl Player {
    /// Hooks the player up to the window. `slint::BackendSelector` must
    /// already have been told to use OpenGL.
    pub fn start(ui: &AppWindow, store: Rc<Store>) -> Result<Rc<Self>, slint::SetRenderingNotifierError> {
        let wake = {
            let ui = ui.as_weak();
            move || {
                let _ = ui.upgrade_in_event_loop(|ui| ui.window().request_redraw());
            }
        };
        let store_preferences = store.get::<Preferences>(PREFERENCES).ok().flatten();
        let player = Rc::new(Self {
            ui: ui.as_weak(),
            store,
            video: RefCell::new(Video::new(wake)),
            playing: RefCell::default(),
            seek_held: Cell::new(None),
            controls_shown: Cell::new(true),
            margin_set: Cell::new(None),
            audio: Rc::default(),
            subtitles: Rc::default(),
            tracks_shown: RefCell::default(),
            unusable: RefCell::default(),
            closed: RefCell::default(),
            next: RefCell::default(),
            next_at: Cell::new(None),
            preferences: Cell::new(
                store_preferences.unwrap_or_default(),
            ),
            speed: Cell::new(1.0),
            fit: Cell::new(0),
            subtitle_delay: Cell::new(0.0),
            ticker: Timer::default(),
        });
        player.show_settings();
        ui.set_player_audio(ModelRc::from(player.audio.clone()));
        ui.set_player_subtitles(ModelRc::from(player.subtitles.clone()));

        ui.window().set_rendering_notifier({
            let player = player.clone();
            move |state, api| {
                let ui = player.ui.upgrade();
                let size = ui.as_ref().map_or((1, 1), |ui| {
                    let size = ui.window().size();
                    (size.width, size.height)
                });
                let mut show = |frame: Image| {
                    if let Some(ui) = &ui {
                        ui.set_player_frame(frame);
                    }
                };
                let step = player.video.borrow_mut().rendering(state, api, size, &mut show);
                if let Err(err) = step {
                    eprintln!("haven: the player cannot work here: {err}");
                    *player.unusable.borrow_mut() = Some(err);
                    // Not from inside the renderer: show it on the next turn.
                    let weak = Rc::downgrade(&player);
                    Timer::single_shot(Duration::ZERO, move || {
                        if let Some(player) = weak.upgrade() {
                            player.show_unusable();
                        }
                    });
                }
            }
        })?;

        macro_rules! connect {
            ($on:ident, |$($arg:ident),*| $call:expr) => {
                ui.$on({
                    let player = Rc::downgrade(&player);
                    move |$($arg),*| {
                        if let Some(player) = player.upgrade() {
                            $call(&player)
                        }
                    }
                });
            };
        }
        connect!(on_player_close, | | |p: &Rc<Self>| p.close());
        connect!(on_player_toggle_pause, | | |p: &Rc<Self>| p.toggle_pause());
        connect!(on_player_seek, |seconds| |p: &Rc<Self>| p.seek_to(seconds as f64));
        connect!(on_player_skip, |seconds| |p: &Rc<Self>| p.skip(seconds as f64));
        connect!(on_player_choose_audio, |id| |p: &Rc<Self>| p.choose_audio(id));
        connect!(on_player_choose_subtitle, |id| |p: &Rc<Self>| p.choose_subtitle(id));
        connect!(on_player_toggle_fullscreen, | | |p: &Rc<Self>| p.toggle_fullscreen());
        connect!(on_player_set_volume, |volume| |p: &Rc<Self>| p.set_volume(volume as f64));
        connect!(on_player_toggle_mute, | | |p: &Rc<Self>| p.toggle_mute());
        connect!(on_player_set_speed, |speed| |p: &Rc<Self>| p.set_speed(speed as f64));
        connect!(on_player_set_fit, |fit| |p: &Rc<Self>| p.set_fit(fit));
        connect!(on_player_subtitle_scale_step, |way| |p: &Rc<Self>| p.step_subtitle_scale(way));
        connect!(on_player_subtitle_delay_step, |way| |p: &Rc<Self>| p.step_subtitle_delay(way));
        connect!(on_player_next, | | |p: &Rc<Self>| p.go_next());
        connect!(on_player_cancel_next, | | |p: &Rc<Self>| p.close());
        connect!(on_player_controls_shown, |shown| |p: &Rc<Self>| {
            p.controls_shown.set(shown);
            p.place_subtitles();
        });

        let weak = Rc::downgrade(&player);
        player.ticker.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            if let Some(player) = weak.upgrade() {
                player.tick();
            }
        });
        Ok(player)
    }

    /// `closed` runs each time the player page closes, with how far the
    /// film or episode got (nothing for a channel or a failed stream).
    pub fn on_closed(&self, closed: impl Fn(Option<Played>) + 'static) {
        *self.closed.borrow_mut() = Some(Box::new(closed));
    }

    /// `next` runs when the episode after the one playing is wanted (the
    /// button, the key, or the end of the episode). It is given how far
    /// the one that was playing got, and says whether a next episode is
    /// on its way.
    pub fn on_next(&self, next: impl Fn(Option<Played>) -> bool + 'static) {
        *self.next.borrow_mut() = Some(Box::new(next));
    }

    /// Shows the volume, speed and the like on the controls.
    fn show_settings(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let preferences = self.preferences.get();
        ui.set_player_volume(preferences.volume as f32);
        ui.set_player_muted(preferences.muted);
        ui.set_player_subtitle_scale(preferences.subtitle_scale as f32);
        ui.set_player_speed(self.speed.get() as f32);
        ui.set_player_fit(self.fit.get());
        ui.set_player_subtitle_delay(self.subtitle_delay.get() as f32);
    }

    /// Gives mpv the volume, speed and the like (it forgets nothing
    /// between files, but it only exists once the first one is asked for).
    fn apply_settings(&self) {
        let video = self.video.borrow();
        let preferences = self.preferences.get();
        video.set_volume(preferences.volume);
        video.set_muted(preferences.muted);
        video.set_subtitle_scale(preferences.subtitle_scale);
        video.set_speed(self.speed.get());
        video.set_fit(fit_of(self.fit.get()));
        video.set_subtitle_delay(self.subtitle_delay.get());
    }

    fn change_preferences(&self, change: impl FnOnce(&mut Preferences)) {
        let mut preferences = self.preferences.get();
        change(&mut preferences);
        self.preferences.set(preferences);
        if let Err(err) = self.store.put(PREFERENCES, &preferences) {
            eprintln!("haven: could not save the player's settings: {err}");
        }
        self.apply_settings();
        self.show_settings();
    }

    fn set_volume(&self, volume: f64) {
        // Moving the volume turns the sound back on.
        self.change_preferences(|preferences| {
            preferences.volume = volume.clamp(0.0, 1.0);
            preferences.muted = false;
        });
    }

    fn toggle_mute(&self) {
        self.change_preferences(|preferences| preferences.muted = !preferences.muted);
    }

    fn step_subtitle_scale(&self, way: i32) {
        self.change_preferences(|preferences| {
            preferences.subtitle_scale =
                (preferences.subtitle_scale + 0.1 * f64::from(way.signum())).clamp(0.5, 2.5);
        });
    }

    fn step_subtitle_delay(&self, way: i32) {
        self.subtitle_delay
            .set((self.subtitle_delay.get() + 0.1 * f64::from(way.signum())).clamp(-30.0, 30.0));
        self.apply_settings();
        self.show_settings();
    }

    fn set_speed(&self, speed: f64) {
        self.speed.set(speed.clamp(0.25, 4.0));
        self.apply_settings();
        self.show_settings();
    }

    fn set_fit(&self, fit: i32) {
        self.fit.set(fit.clamp(0, 2));
        self.apply_settings();
        self.show_settings();
    }

    /// Opens the player page and plays the stream, from where it was left
    /// if it was left partway.
    pub fn play(&self, request: PlayRequest) {
        let Some(ui) = self.ui.upgrade() else { return };
        let at = Some(&request)
            .filter(|request| request.remember)
            .and_then(|request| {
                self.store
                    .progress(&request.kind, &request.title_id, &request.video_id)
                    .transpose()
            })
            .transpose()
            .unwrap_or_else(|err| {
                eprintln!("haven: could not read where this was left: {err}");
                None
            })
            .and_then(|saved| resume::resume_from(saved.position, saved.duration, saved.finished));

        ui.set_player_title(request.title.as_str().into());
        ui.set_player_subtitle(request.detail.as_str().into());
        ui.set_player_error("".into());
        ui.set_player_busy(true);
        ui.set_player_paused(false);
        ui.set_player_position(at.unwrap_or(0.0) as f32);
        ui.set_player_duration(0.0);
        ui.set_player_time("".into());
        ui.set_player_elapsed("".into());
        ui.set_player_remaining("".into());
        ui.set_player_ends_at("".into());
        ui.set_player_info("".into());
        ui.set_player_next_title(request.next.as_deref().unwrap_or_default().into());
        ui.set_player_next_countdown(-1);
        self.next_at.set(None);
        // Each film or episode has its own subtitle timing.
        self.subtitle_delay.set(0.0);
        self.show_settings();
        ui.set_player_any_subtitle(false);
        ui.set_player_menu(0);
        self.audio.set_vec(Vec::new());
        self.subtitles.set_vec(Vec::new());
        self.tracks_shown.borrow_mut().clear();
        self.seek_held.set(None);
        ui.set_player_open(true);

        let address = request.address.clone();
        *self.playing.borrow_mut() = Some(Playing {
            request,
            loaded: false,
            failed: false,
            position: at.unwrap_or(0.0),
            duration: 0.0,
            saved_at: Instant::now(),
        });
        if self.unusable.borrow().is_some() {
            return self.show_unusable();
        }
        self.video.borrow_mut().play(&address, at);
    }

    fn show_unusable(&self) {
        let reason = self.unusable.borrow().clone().unwrap_or_default();
        self.fail(&format!("Haven's player cannot start on this computer. ({reason})"));
    }

    fn fail(&self, message: &str) {
        if let Some(playing) = self.playing.borrow_mut().as_mut() {
            playing.failed = true;
        } else {
            return;
        }
        self.video.borrow_mut().stop();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_player_frame(Image::default());
            ui.set_player_busy(false);
            ui.set_player_error(message.into());
        }
    }

    /// Four times a second while the page is open: mpv's news, the
    /// position, the track menus, and saving the place now and then.
    fn tick(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        // The episode is over and the next one is counting down.
        if let Some(at) = self.next_at.get() {
            let left = at.saturating_duration_since(Instant::now());
            ui.set_player_next_countdown(left.as_secs_f64().ceil() as i32);
            if left.is_zero() {
                self.go_next();
            }
            return;
        }
        if self.playing.borrow().as_ref().is_none_or(|playing| playing.failed) {
            return;
        }
        let events = self.video.borrow_mut().poll();
        for event in events {
            match event {
                Event::Loaded => {
                    if let Some(playing) = self.playing.borrow_mut().as_mut() {
                        playing.loaded = true;
                    }
                    self.apply_settings();
                }
                Event::Finished => return self.finished(),
                Event::Failed(reason) => {
                    // What was watched before it broke off still counts.
                    self.save();
                    return self.fail(&format!("This stream could not be played. ({reason})"));
                }
            }
        }
        let Some(status) = self.video.borrow().status() else { return };
        let loaded = {
            let mut playing = self.playing.borrow_mut();
            let Some(playing) = playing.as_mut() else { return };
            if playing.loaded {
                playing.position = status.position;
                playing.duration = status.duration;
            }
            playing.loaded
        };
        ui.set_player_busy(!loaded || status.buffering);
        if !loaded {
            return;
        }
        ui.set_player_paused(status.paused);
        ui.set_player_duration(status.duration.max(0.0) as f32);
        ui.set_player_time(
            if status.duration > 0.0 {
                format!("{} / {}", clock(status.position), clock(status.duration))
            } else {
                clock(status.position)
            }
            .into(),
        );
        let left = (status.duration - status.position).max(0.0);
        ui.set_player_elapsed(clock(status.position).into());
        ui.set_player_remaining(
            if status.duration > 0.0 { format!("−{}", clock(left)) } else { "Live".to_owned() }.into(),
        );
        ui.set_player_ends_at(
            if status.duration > 0.0 {
                let wait = left / self.speed.get().max(0.25);
                let ends = chrono::Local::now() + chrono::Duration::seconds(wait as i64);
                format!("Ends at {}", ends.format("%H:%M"))
            } else {
                String::new()
            }
            .into(),
        );
        if ui.get_player_menu() == 5 {
            ui.set_player_info(self.info().into());
        }
        if self.seek_held.get().is_none_or(|at| at.elapsed() >= SEEK_HOLD) {
            ui.set_player_position(status.position as f32);
        }
        self.show_tracks(&ui);
        self.place_subtitles();

        let due = self
            .playing
            .borrow()
            .as_ref()
            .is_some_and(|playing| playing.saved_at.elapsed() >= SAVE_EVERY);
        if due && !status.paused {
            self.save();
        }
    }

    /// Keeps subtitles clear of the controls while those are on screen.
    fn place_subtitles(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let margin = if self.controls_shown.get() {
            let window = ui.window();
            let height = window.size().height as f32 / window.scale_factor();
            // mpv measures the margin as if the picture were 720 high.
            (CONTROLS_HEIGHT * 720.0 / height.max(1.0)).round() as i64
        } else {
            SUBTITLE_MARGIN
        };
        if self.margin_set.get() != Some(margin) && self.video.borrow().status().is_some() {
            self.video.borrow().set_subtitle_margin(margin);
            self.margin_set.set(Some(margin));
        }
    }

    fn show_tracks(&self, ui: &AppWindow) {
        let json = self.video.borrow().tracks_json();
        if json == *self.tracks_shown.borrow() {
            return;
        }
        let tracks = haven_player::tracks::parse_tracks(&json);
        *self.tracks_shown.borrow_mut() = json;
        let rows = |kind: TrackKind| -> Vec<TrackRow> {
            tracks
                .iter()
                .filter(|track| track.kind == kind)
                .map(|track| TrackRow {
                    id: track.id as i32,
                    label: track.label.as_str().into(),
                    selected: track.selected,
                })
                .collect()
        };
        let subtitles = rows(TrackKind::Subtitle);
        ui.set_player_any_subtitle(subtitles.iter().any(|row| row.selected));
        self.audio.set_vec(rows(TrackKind::Audio));
        self.subtitles.set_vec(subtitles);
    }

    /// Saves how far the current film or episode has been watched, if
    /// that is worth remembering.
    fn save(&self) {
        let mut playing = self.playing.borrow_mut();
        let Some(playing) = playing.as_mut() else { return };
        playing.saved_at = Instant::now();
        if !playing.request.remember {
            return;
        }
        let finished = match resume::judge(playing.position, playing.duration) {
            Some(Watched::Partway) => false,
            Some(Watched::Finished) => true,
            Some(Watched::Started) | None => return,
        };
        let request = &playing.request;
        let saved = self.store.save_progress(&Progress {
            kind: request.kind.clone(),
            title_id: request.title_id.clone(),
            video_id: request.video_id.clone(),
            title: request.title.clone(),
            detail: request.detail.clone(),
            poster: request.poster.clone(),
            position: playing.position,
            duration: playing.duration,
            finished,
            updated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_millis() as i64),
        });
        if let Err(err) = saved {
            eprintln!("haven: could not save where this was left: {err}");
        }
    }

    /// What is known of the picture and sound, a fact a line.
    fn info(&self) -> String {
        let details = self.video.borrow().details();
        let mut lines = Vec::new();
        if let (Some(width), Some(height)) = (details.width, details.height) {
            let rate = details
                .frames_per_second
                .map(|rate| format!(", {} frames a second", (rate * 1000.0).round() / 1000.0))
                .unwrap_or_default();
            lines.push(format!("Picture: {width} × {height}{rate}"));
        }
        if let Some(video) = details.video {
            let by = details.decoder.map(|decoder| format!(" (decoded by {decoder})")).unwrap_or_default();
            lines.push(format!("Video: {video}{by}"));
        }
        if let Some(audio) = details.audio {
            let channels = details.channels.map(|channels| format!(", {channels}")).unwrap_or_default();
            lines.push(format!("Sound: {audio}{channels}"));
        }
        lines.extend(details.container.map(|container| format!("File: {container}")));
        if let Some(playing) = self.playing.borrow().as_ref() {
            if !playing.request.source.is_empty() {
                lines.push(format!("From: {}", playing.request.source));
            }
        }
        lines.join("\n")
    }

    /// Played to the end: it counts as watched. An episode with another
    /// after it counts down to that one; anything else closes the page.
    fn finished(&self) {
        let has_next = {
            let mut playing = self.playing.borrow_mut();
            let Some(playing) = playing.as_mut() else { return };
            if playing.duration > 0.0 {
                playing.position = playing.duration;
            }
            playing.request.next.is_some()
        };
        if !has_next || self.next.borrow().is_none() {
            return self.close();
        }
        self.save();
        self.next_at.set(Some(Instant::now() + NEXT_AFTER));
        if let Some(ui) = self.ui.upgrade() {
            ui.set_player_busy(false);
            ui.set_player_paused(true);
            ui.set_player_next_countdown(NEXT_AFTER.as_secs() as i32);
        }
    }

    /// How far what was playing got, for the books; takes it off the player.
    fn take_played(&self) -> Option<Played> {
        self.playing.borrow_mut().take().and_then(|playing| {
            let finished = match resume::judge(playing.position, playing.duration)? {
                Watched::Finished => true,
                Watched::Partway => false,
                Watched::Started => return None,
            };
            (playing.request.remember && !playing.failed).then(|| Played {
                kind: playing.request.kind,
                title_id: playing.request.title_id,
                video_id: playing.request.video_id,
                position: playing.position,
                duration: playing.duration,
                finished,
            })
        })
    }

    /// Moves on to the next episode without leaving the player. If there
    /// turns out to be none, the page closes.
    fn go_next(&self) {
        if self.playing.borrow().is_none() {
            return;
        }
        self.next_at.set(None);
        self.save();
        let played = self.take_played();
        self.video.borrow_mut().stop();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_player_next_countdown(-1);
            ui.set_player_menu(0);
            ui.set_player_frame(Image::default());
            ui.set_player_subtitle(ui.get_player_next_title());
            ui.set_player_next_title("".into());
            ui.set_player_error("".into());
            ui.set_player_busy(true);
            ui.set_player_duration(0.0);
            ui.set_player_position(0.0);
            ui.set_player_elapsed("".into());
            ui.set_player_remaining("".into());
            ui.set_player_ends_at("".into());
        }
        let coming = self.next.borrow().as_ref().is_some_and(|next| next(played));
        if !coming {
            self.dismiss();
        }
    }

    /// Leaves the player page when nothing is playing (the next episode
    /// could not be started by itself).
    pub fn dismiss(&self) {
        if self.playing.borrow().is_some() {
            return;
        }
        self.next_at.set(None);
        if let Some(ui) = self.ui.upgrade() {
            if ui.window().is_fullscreen() {
                ui.window().set_fullscreen(false);
            }
            ui.set_player_fullscreen(false);
            ui.set_player_open(false);
            ui.set_player_frame(Image::default());
        }
    }

    /// Saves the place, stops and goes back to the page underneath.
    pub fn close(&self) {
        if self.playing.borrow().is_none() {
            return;
        }
        self.save();
        self.next_at.set(None);
        let played = self.take_played();
        self.video.borrow_mut().stop();
        if let Some(ui) = self.ui.upgrade() {
            if ui.window().is_fullscreen() {
                ui.window().set_fullscreen(false);
            }
            ui.set_player_fullscreen(false);
            ui.set_player_open(false);
            ui.set_player_frame(Image::default());
        }
        if let Some(closed) = self.closed.borrow().as_ref() {
            closed(played);
        }
    }

    fn toggle_pause(&self) {
        let video = self.video.borrow();
        let Some(status) = video.status() else { return };
        video.set_paused(!status.paused);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_player_paused(!status.paused);
        }
        drop(video);
        // Pausing is a good moment to remember the place.
        if !status.paused {
            self.save();
        }
    }

    fn seek_to(&self, seconds: f64) {
        self.video.borrow().seek_to(seconds);
        self.hold_position(seconds);
    }

    fn skip(&self, seconds: f64) {
        let video = self.video.borrow();
        let Some(status) = video.status() else { return };
        video.seek_by(seconds);
        let mut target = (status.position + seconds).max(0.0);
        if status.duration > 0.0 {
            target = target.min(status.duration);
        }
        drop(video);
        self.hold_position(target);
    }

    fn hold_position(&self, seconds: f64) {
        self.seek_held.set(Some(Instant::now()));
        if let Some(ui) = self.ui.upgrade() {
            ui.set_player_position(seconds as f32);
        }
    }

    fn choose_audio(&self, id: i32) {
        self.video.borrow().select_audio(id as i64);
    }

    /// 0 turns subtitles off.
    fn choose_subtitle(&self, id: i32) {
        self.video.borrow().select_subtitle((id > 0).then_some(id as i64));
    }

    fn toggle_fullscreen(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let fullscreen = !ui.window().is_fullscreen();
        ui.window().set_fullscreen(fullscreen);
        ui.set_player_fullscreen(fullscreen);
    }

    /// Call when the event loop has ended, before the window is dropped:
    /// saves the place of whatever is still playing.
    pub fn leaving(&self) {
        self.save();
        *self.playing.borrow_mut() = None;
    }

    /// Call after the window is gone: waits for mpv to end.
    pub fn finish(&self) {
        self.video.borrow_mut().finish();
    }
}

fn fit_of(choice: i32) -> Fit {
    match choice {
        1 => Fit::Fill,
        2 => Fit::Stretch,
        _ => Fit::Whole,
    }
}

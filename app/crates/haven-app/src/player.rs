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
    video::{Event, Video},
};
use slint::{ComponentHandle, Image, ModelRc, Timer, TimerMode, VecModel};

use crate::{AppWindow, TrackRow, library::PlayRequest};

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
    closed: RefCell<Option<Box<dyn Fn()>>>,
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
            ticker: Timer::default(),
        });
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

    /// `closed` runs each time the player page closes.
    pub fn on_closed(&self, closed: impl Fn() + 'static) {
        *self.closed.borrow_mut() = Some(Box::new(closed));
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
        if self.playing.borrow().as_ref().is_none_or(|playing| playing.failed) {
            return;
        }
        let Some(ui) = self.ui.upgrade() else { return };
        let events = self.video.borrow_mut().poll();
        for event in events {
            match event {
                Event::Loaded => {
                    if let Some(playing) = self.playing.borrow_mut().as_mut() {
                        playing.loaded = true;
                    }
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

    /// Played to the end: it counts as watched, and the page closes.
    fn finished(&self) {
        if let Some(playing) = self.playing.borrow_mut().as_mut() {
            if playing.duration > 0.0 {
                playing.position = playing.duration;
            }
        }
        self.close();
    }

    /// Saves the place, stops and goes back to the page underneath.
    pub fn close(&self) {
        if self.playing.borrow().is_none() {
            return;
        }
        self.save();
        *self.playing.borrow_mut() = None;
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
            closed();
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

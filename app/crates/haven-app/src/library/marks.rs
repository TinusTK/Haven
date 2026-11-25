//! Marking things watched or unwatched by hand: a film, one episode, or
//! a whole season of the open title. The mark is kept on this device
//! and, through sync, sent to the linked accounts.

use std::rc::Rc;

use haven_titles::Video;
use haven_core::storage::Progress;

use super::Library;
use crate::AppWindow;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// "S1 E2 · Its name", as playing an episode records it.
fn episode_detail(video: &Video) -> String {
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

/// Has come out by `today` (an ISO date); no date counts as out.
fn aired(video: &Video, today: &str) -> bool {
    video.released.as_deref().is_none_or(|day| day.get(..10).unwrap_or(day) <= today)
}

impl Library {
    pub(super) fn start_marks(self: &Rc<Self>, ui: &AppWindow) {
        ui.on_episode_watched({
            let library = self.clone();
            move |id, on| library.mark_episodes(&[id.to_string()], on)
        });
        ui.on_season_watched({
            let library = self.clone();
            move |on| library.mark_season(on)
        });
        ui.on_film_watched({
            let library = self.clone();
            move |on| library.mark_film(on)
        });
    }

    /// Marks one film or episode, here and (by sync) on the accounts.
    #[allow(clippy::too_many_arguments)]
    fn mark_one(&self, kind: &str, title_id: &str, video_id: &str, title: &str, detail: &str, poster: Option<String>, on: bool) {
        let done = if on {
            // Something partly watched becomes finished; anything else
            // already on record keeps its poster.
            self.store.save_progress(&Progress {
                kind: kind.to_owned(),
                title_id: title_id.to_owned(),
                video_id: video_id.to_owned(),
                title: title.to_owned(),
                detail: detail.to_owned(),
                poster,
                position: 0.0,
                duration: 0.0,
                finished: true,
                updated_at: now_ms(),
            })
        } else {
            // A tick that came from a service under the IMDb id goes too.
            self.forget_linked(kind, title_id, video_id);
            self.store.remove_video_progress(kind, title_id, video_id)
        };
        match done {
            Ok(()) => self.queue_sync(if on { "watched" } else { "unwatched" }, kind, title_id, video_id),
            Err(err) => eprintln!("haven: could not mark it {}: {err}", if on { "watched" } else { "unwatched" }),
        }
    }

    /// The open title's kind, id, name and poster.
    fn open_title_facts(&self) -> Option<(String, String, String, Option<String>)> {
        let open = self.open.borrow();
        let open = open.as_ref()?;
        let name = open.meta.as_ref().map(|meta| meta.name.clone()).unwrap_or_else(|| {
            self.ui.upgrade().map(|ui| ui.get_detail().title.to_string()).unwrap_or_default()
        });
        let poster = open.meta.as_ref().and_then(|meta| meta.poster.clone());
        Some((open.kind.clone(), open.id.clone(), name, poster))
    }

    /// Marks episodes of the open series by their ids.
    fn mark_episodes(&self, ids: &[String], on: bool) {
        let Some((kind, title_id, name, poster)) = self.open_title_facts() else { return };
        let videos: Vec<Video> = self
            .open
            .borrow()
            .as_ref()
            .and_then(|open| open.meta.as_ref())
            .map(|meta| meta.videos.iter().filter(|video| ids.contains(&video.id)).cloned().collect())
            .unwrap_or_default();
        for video in &videos {
            self.mark_one(&kind, &title_id, &video.id, &name, &episode_detail(video), poster.clone(), on);
        }
        self.after_marking();
    }

    /// Marks every episode of the season showing (unwatched: all of them;
    /// watched: those that have come out).
    fn mark_season(&self, on: bool) {
        let Some(season) = self.ui.upgrade().map(|ui| ui.get_season()) else { return };
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let ids: Vec<String> = self
            .open
            .borrow()
            .as_ref()
            .and_then(|open| open.meta.as_ref())
            .map(|meta| {
                meta.episodes(season.max(0) as u32)
                    .into_iter()
                    .filter(|video| !on || aired(video, &today))
                    .map(|video| video.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        self.mark_episodes(&ids, on);
    }

    /// Marks the open film.
    fn mark_film(&self, on: bool) {
        let Some((kind, title_id, name, poster)) = self.open_title_facts() else { return };
        if on {
            self.mark_one(&kind, &title_id, &title_id, &name, "", poster, true);
        } else {
            // Whatever was recorded for the film (its id may differ).
            for video in self.finished_of(&kind, &title_id) {
                self.mark_one(&kind, &title_id, &video, &name, "", None, false);
            }
            let _ = self.store.remove_video_progress(&kind, &title_id, &title_id);
        }
        self.after_marking();
    }

    /// Puts every tick, Continue Watching and History right again.
    fn after_marking(&self) {
        self.refresh_ticks();
        self.reload_continue();
        self.reload_watchlist();
        self.refresh_shelf_ticks();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(id: &str, season: u32, episode: u32, released: Option<&str>) -> Video {
        Video {
            id: id.to_owned(),
            title: Some("Pilot".to_owned()),
            season: Some(season),
            episode: Some(episode),
            released: released.map(str::to_owned),
            thumbnail: None,
        }
    }

    #[test]
    fn an_episode_is_described_as_playing_it_records() {
        assert_eq!(episode_detail(&video("x:1:2", 1, 2, None)), "S1 E2 · Pilot");
    }

    #[test]
    fn only_aired_episodes_count_as_out() {
        assert!(aired(&video("a", 1, 1, Some("2020-01-01T00:00:00.000Z")), "2026-11-22"));
        assert!(!aired(&video("a", 1, 1, Some("2027-01-01")), "2026-11-22"));
        assert!(aired(&video("a", 1, 1, None), "2026-11-22"));
    }
}

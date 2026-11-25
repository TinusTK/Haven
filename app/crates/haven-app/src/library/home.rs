//! The top of Home: a large banner that turns through a few titles of
//! the first shelf to load (picture, name, facts, a line of story), and
//! the Upcoming row of episodes coming out from the Watchlist.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use haven_titles::{MetaDetail, MetaPreview};
use slint::{Image, Model, ModelRc, VecModel};

use super::{BACKGROUND_SIZE, Library, on_ui};
use crate::{ContinueCard, HeroSlide, UpcomingCard};

/// Titles the banner turns through.
const SLIDES: usize = 8;
/// A wide picture for an Upcoming card.
const CARD_SIZE: (u32, u32) = (640, 360);

#[derive(Default)]
pub(super) struct HomeTop {
    started: Cell<bool>,
    /// Banner titles whose details have answered (or failed).
    settled: Cell<usize>,
    slides: Rc<VecModel<HeroSlide>>,
    pub(super) upcoming: Rc<VecModel<UpcomingCard>>,
    /// The wide picture of each Watchlist title, once loaded.
    pictures: RefCell<HashMap<(String, String), Image>>,
    /// Pictures asked for already.
    asked: RefCell<std::collections::HashSet<(String, String)>>,
    continuing: Rc<VecModel<ContinueCard>>,
    /// What is known of each title on Continue Watching: its wide
    /// picture's address and its episodes (asked for once a run).
    continue_meta: RefCell<HashMap<(String, String), Option<(Option<String>, Vec<haven_titles::Video>)>>>,
    /// Titles whose details have been asked for this run (kept details
    /// from last time are shown until the fresh ones come).
    continue_asked: RefCell<std::collections::HashSet<(String, String)>>,
}

/// "S03E06" from an episode id such as `tt123:3:6`.
pub(super) fn episode_badge(video_id: &str) -> String {
    let mut parts = video_id.rsplit(':');
    match (parts.next().and_then(|e| e.parse::<u32>().ok()), parts.next().and_then(|s| s.parse::<u32>().ok())) {
        (Some(episode), Some(season)) => format!("S{season:02}E{episode:02}"),
        _ => String::new(),
    }
}

/// How many aired episodes are still to watch, from `current` on (a
/// series' regular episodes, in order; specials left out).
pub(super) fn episodes_left(
    videos: &[haven_titles::Video],
    current: &str,
    finished: &std::collections::HashSet<String>,
    today: &str,
) -> i32 {
    let mut episodes: Vec<&haven_titles::Video> = videos
        .iter()
        .filter(|video| video.season.unwrap_or(1) > 0 && video.episode.is_some())
        .filter(|video| video.released.as_deref().is_some_and(|day| day.get(..10).unwrap_or(day) <= today))
        .collect();
    episodes.sort_by_key(|video| (video.season.unwrap_or(1), video.episode.unwrap_or(0)));
    let Some(at) = episodes.iter().position(|video| video.id == current) else { return 0 };
    episodes[at..].iter().filter(|video| !finished.contains(&video.id)).count() as i32
}

/// "★ 8.2  •  2026  •  Horror, Thriller  •  Film" from a title's details.
/// The width a banner's title logo is fetched at.
const LOGO_SIZE: (u32, u32) = (600, 240);

/// The line under the banner's title: "2026 · Adventure, Action, Fantasy
/// · 173 min · R".
pub(super) fn banner_facts(meta: &MetaDetail, scores: Option<&haven_metadata::omdb::Scores>) -> String {
    let mut parts = Vec::new();
    if let Some(year) = meta.release_info.as_deref().filter(|y| !y.is_empty()) {
        parts.push(year.to_owned());
    }
    if !meta.genres.is_empty() {
        parts.push(meta.genres.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
    }
    let runtime = meta.runtime.clone().or_else(|| scores.and_then(|s| s.runtime.clone()));
    if let Some(runtime) = runtime.filter(|r| !r.is_empty()) {
        parts.push(runtime);
    }
    if let Some(rated) = scores.and_then(|s| s.rated.clone()) {
        parts.push(rated);
    }
    parts.join("  ·  ")
}

/// The scores under the banner's title, those that are known.
pub(super) fn score_pills(
    meta: &MetaDetail,
    tmdb: Option<f64>,
    scores: Option<&haven_metadata::omdb::Scores>,
) -> Vec<crate::ScorePill> {
    let mut pills = Vec::new();
    let mut add = |label: &str, value: String, tone: i32| {
        pills.push(crate::ScorePill { label: label.into(), value: value.into(), tone });
    };
    if let Some(imdb) = meta.rating.clone().or_else(|| scores.and_then(|s| s.imdb.clone())).filter(|r| !r.is_empty()) {
        add("IMDb", imdb, 0);
    }
    if let Some(tmdb) = tmdb.filter(|r| *r > 0.0) {
        add("TMDB", format!("{tmdb:.1}"), 1);
    }
    if let Some(rt) = scores.and_then(|s| s.rotten_tomatoes.clone()) {
        add("RT", rt, 2);
    }
    if let Some(mc) = scores.and_then(|s| s.metacritic.clone()) {
        add("MC", mc, 3);
    }
    pills
}


impl Library {
    pub(super) fn start_home_top(&self, ui: &crate::AppWindow) {
        ui.set_continue_cards(ModelRc::from(self.home_top.continuing.clone()));
        ui.set_hero(ModelRc::from(self.home_top.slides.clone()));
        ui.set_upcoming(ModelRc::from(self.home_top.upcoming.clone()));
    }

    /// Asks Continue Watching's titles' details again (Home pulled down).
    pub(super) fn recheck_continue(&self) {
        self.home_top.continue_asked.borrow_mut().clear();
    }

    /// Fills the banner from the first shelf that loads with titles.
    pub(super) fn offer_hero(&self, metas: &[MetaPreview]) {
        if self.home_top.started.get() || metas.len() < 3 {
            return;
        }
        self.home_top.started.set(true);
        let chosen: Vec<&MetaPreview> = metas.iter().take(SLIDES).collect();
        self.home_top.slides.set_vec(
            chosen
                .iter()
                .map(|meta| HeroSlide {
                    id: meta.id.as_str().into(),
                    kind: meta.kind.as_str().into(),
                    title: meta.name.as_str().into(),
                    facts: meta.release_info.as_deref().unwrap_or_default().into(),
                    ..HeroSlide::default()
                })
                .collect::<Vec<_>>(),
        );
        for (slot, meta) in chosen.into_iter().enumerate() {
            // Details kept from last time show at once; fresh ones follow.
            let kept = self.kept_details(&meta.kind, &meta.id);
            if let Some(kept) = &kept {
                self.home_top.settled.set(self.home_top.settled.get() + 1);
                self.hero_details(slot, kept.clone());
            }
            let Some(details) = self.details_of(&meta.kind, &meta.id) else {
                if kept.is_none() {
                    self.home_top.settled.set(self.home_top.settled.get() + 1);
                }
                continue;
            };
            self.tasks.spawn(async move {
                let found = details.await;
                on_ui(move |library| {
                    if kept.is_none() {
                        library.home_top.settled.set(library.home_top.settled.get() + 1);
                    }
                    if let Ok(found) = found {
                        if Some(&found) != kept.as_ref() {
                            library.hero_details(slot, found);
                        }
                    }
                });
            });
        }
    }

    fn hero_details(&self, slot: usize, meta: MetaDetail) {
        let Some(mut slide) = self.home_top.slides.row_data(slot) else { return };
        slide.facts = banner_facts(&meta, None).into();
        slide.description = meta.description.as_deref().unwrap_or_default().into();
        slide.scores = ModelRc::new(VecModel::from(score_pills(&meta, None, None)));
        self.home_top.slides.set_row_data(slot, slide);
        self.hero_scores(slot, &meta);
        if let Some(url) = meta.logo.clone() {
            self.fetch_image_for(Some(self.home_pause.clone()), url, LOGO_SIZE, move |library, pixels| {
                if let Some(mut slide) = library.home_top.slides.row_data(slot) {
                    slide.logo = Image::from_rgba8(pixels);
                    slide.has_logo = true;
                    library.home_top.slides.set_row_data(slot, slide);
                }
            });
        }
        if let Some(url) = meta.background.or(meta.poster) {
            self.fetch_image_for(Some(self.home_pause.clone()), url, BACKGROUND_SIZE, move |library, pixels| {
                if let Some(mut slide) = library.home_top.slides.row_data(slot) {
                    slide.image = Image::from_rgba8(pixels);
                    slide.has_image = true;
                    library.home_top.slides.set_row_data(slot, slide);
                }
            });
        }
    }

    /// Adds the TMDB, Rotten Tomatoes and Metacritic scores (and the age
    /// rating) to a banner slide, where keys for them are set. OMDb's
    /// answers are kept for a week: its free key allows 1,000 a day.
    fn hero_scores(&self, slot: usize, meta: &MetaDetail) {
        let imdb = meta.imdb_id.clone().or_else(|| haven_metadata::is_imdb_id(&meta.id).then(|| meta.id.clone()));
        let Some(imdb) = imdb else { return };
        let (tmdb_key, omdb_key) = {
            let keys = self.keys.borrow();
            (keys.tmdb_api_key.clone(), keys.omdb_api_key.clone())
        };
        let kept_name = format!("omdb|{imdb}");
        let kept: Option<(i64, haven_metadata::omdb::Scores)> = self.kept(&kept_name);
        let fresh_enough = kept.as_ref().is_some_and(|(at, _)| chrono::Utc::now().timestamp() - at < 7 * 86_400);
        let (http, series, meta) = (self.http.clone(), meta.kind == "series", meta.clone());
        let dir = self.kept_dir();
        self.tasks.spawn(async move {
            let tmdb = match tmdb_key {
                Some(key) => haven_metadata::details(&http, &key, series, &imdb).await.ok().flatten().and_then(|d| d.rating),
                None => None,
            };
            let mut scores = kept.map(|(_, scores)| scores);
            if !fresh_enough {
                if let Some(key) = omdb_key {
                    if let Ok(Some(found)) = haven_metadata::omdb::scores(&http, &key, &imdb).await {
                        super::kept::write_kept(&dir, &kept_name, &(chrono::Utc::now().timestamp(), &found));
                        scores = Some(found);
                    }
                }
            }
            on_ui(move |library| {
                let Some(mut slide) = library.home_top.slides.row_data(slot) else { return };
                if slide.id.as_str() != meta.id && slide.title.as_str() != meta.name {
                    return;
                }
                slide.facts = banner_facts(&meta, scores.as_ref()).into();
                slide.scores = ModelRc::new(VecModel::from(score_pills(&meta, tmdb, scores.as_ref())));
                library.home_top.slides.set_row_data(slot, slide);
            });
        });
    }

    /// Puts the coming episodes and films on Home, with each title's
    /// wide picture (asked for once).
    pub(super) fn show_upcoming(&self, cards: Vec<(UpcomingCard, Option<String>)>) {
        let pictures = self.home_top.pictures.borrow();
        let mut wanted = Vec::new();
        let rows: Vec<UpcomingCard> = cards
            .into_iter()
            .map(|(mut card, picture)| {
                let key = (card.kind.to_string(), card.id.to_string());
                if let Some(image) = pictures.get(&key) {
                    card.image = image.clone();
                    card.has_image = true;
                } else if let Some(url) = picture {
                    if self.home_top.asked.borrow_mut().insert(key.clone()) {
                        wanted.push((key, url));
                    }
                }
                card
            })
            .collect();
        drop(pictures);
        self.home_top.upcoming.set_vec(rows);
        for (key, url) in wanted {
            self.fetch_image(url, CARD_SIZE, move |library, pixels| {
                let image = Image::from_rgba8(pixels);
                library.home_top.pictures.borrow_mut().insert(key.clone(), image.clone());
                let cards = &library.home_top.upcoming;
                for at in 0..cards.row_count() {
                    if let Some(mut card) = cards.row_data(at) {
                        if card.kind.as_str() == key.0 && card.id.as_str() == key.1 {
                            card.image = image.clone();
                            card.has_image = true;
                            cards.set_row_data(at, card);
                        }
                    }
                }
            });
        }
    }
}

impl Library {
    /// How far Haven has got with what it loads at start, 0 to 1, for
    /// the loading screen: Home's shelves, the banner's details and its
    /// first picture, and the calendar's dates. A failed request counts
    /// as done, so the screen never waits for an answer that will not come.
    pub(super) fn startup_progress(&self) -> f32 {
        let home_showing = self.ui.upgrade().is_some_and(|ui| ui.get_home_showing());
        let mut parts: Vec<f32> = Vec::new();
        // Home's shelves (they wait while another page shows: not needed then).
        if home_showing {
            let mut total = 0usize;
            let mut done = 0usize;
            for set in [&self.home, &self.server_shelves, &self.catalog] {
                for shelf in set.shelves.iter() {
                    total += 1;
                    if !shelf.loading {
                        done += 1;
                    }
                }
            }
            let has_sources = !self.installed.borrow().is_empty() || !self.servers.borrow().is_empty();
            parts.push(if total == 0 { if has_sources { 0.0 } else { 1.0 } } else { done as f32 / total as f32 });
            // The banner: its titles' details, then the first picture.
            let slides = self.home_top.slides.row_count();
            if self.home_top.started.get() && slides > 0 {
                let details = (self.home_top.settled.get().min(slides)) as f32 / slides as f32;
                let first = self.home_top.slides.row_data(0).is_some_and(|slide| slide.has_image);
                parts.push(details * 0.7 + if first { 0.3 } else { 0.0 });
            } else if total > 0 && done < total {
                parts.push(0.0);
            }
        }
        // The calendar (Home's Upcoming row comes from it).
        let (asked, waiting) = self.calendar_load();
        if asked > 0 {
            parts.push(asked.saturating_sub(waiting) as f32 / asked as f32);
        }
        if parts.is_empty() { 1.0 } else { parts.iter().sum::<f32>() / parts.len() as f32 }
    }

    /// Home's Continue Watching row: wide cards with the title's picture,
    /// the episode, and how many episodes are left to watch.
    pub(super) fn fill_continue_cards(&self, watched: &[haven_core::storage::Progress]) {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        // Details kept from last time, until the fresh ones come.
        for progress in watched {
            let key = (progress.kind.clone(), progress.title_id.clone());
            if !self.home_top.continue_meta.borrow().contains_key(&key) {
                if let Some(meta) = self.kept_details(&key.0, &key.1) {
                    self.home_top.continue_meta.borrow_mut().insert(key, Some((meta.background.clone(), meta.videos.clone())));
                }
            }
        }
        let known = self.home_top.continue_meta.borrow();
        let old: Vec<ContinueCard> = self.home_top.continuing.iter().collect();
        let mut wanted_meta = Vec::new();
        let mut cards = Vec::new();
        for progress in watched {
            let key = (progress.kind.clone(), progress.title_id.clone());
            let (episode_name, badge) = match progress.detail.split_once(" · ") {
                Some((_, name)) => (name.to_owned(), episode_badge(&progress.video_id)),
                None if progress.detail.is_empty() => (String::new(), String::new()),
                None => (String::new(), episode_badge(&progress.video_id)),
            };
            let left = match known.get(&key) {
                Some(Some((_, videos))) if progress.kind == "series" => {
                    let finished = self.finished_of(&progress.kind, &progress.title_id);
                    episodes_left(videos, &progress.video_id, &finished, &today)
                }
                _ => 0,
            };
            if !self.home_top.continue_asked.borrow().contains(&key) {
                wanted_meta.push(key.clone());
            }
            let kept = old.iter().find(|card| card.has_image && card.kind.as_str() == key.0 && card.id.as_str() == key.1);
            cards.push(ContinueCard {
                id: progress.title_id.as_str().into(),
                kind: progress.kind.as_str().into(),
                video: progress.video_id.as_str().into(),
                title: progress.title.as_str().into(),
                episode: if episode_name.is_empty() { badge.clone() } else { episode_name }.into(),
                badge: badge.into(),
                left,
                progress: haven_player::resume::share(progress.position, progress.duration).max(0.02),
                image: kept.map(|card| card.image.clone()).unwrap_or_default(),
                has_image: kept.is_some(),
            });
        }
        drop(known);
        self.home_top.continuing.set_vec(cards);
        // Pictures for cards that have none yet.
        for at in 0..self.home_top.continuing.row_count() {
            let Some(card) = self.home_top.continuing.row_data(at) else { continue };
            if card.has_image {
                continue;
            }
            let key = (card.kind.to_string(), card.id.to_string());
            let picture = self.home_top.continue_meta.borrow().get(&key).cloned().flatten().and_then(|(bg, _)| bg);
            let picture = picture.or_else(|| watched.iter().find(|p| p.kind == key.0 && p.title_id == key.1).and_then(|p| p.poster.clone()));
            if let Some(url) = picture {
                self.fetch_image(url, CARD_SIZE, move |library, pixels| {
                    let image = Image::from_rgba8(pixels);
                    let cards = &library.home_top.continuing;
                    for at in 0..cards.row_count() {
                        if let Some(mut card) = cards.row_data(at) {
                            if card.kind.as_str() == key.0 && card.id.as_str() == key.1 {
                                card.image = image.clone();
                                card.has_image = true;
                                cards.set_row_data(at, card);
                            }
                        }
                    }
                });
            }
        }
        // Details (wide picture, episode list) once per title per run.
        for key in wanted_meta {
            self.home_top.continue_asked.borrow_mut().insert(key.clone());
            if !self.home_top.continue_meta.borrow().contains_key(&key) {
                self.home_top.continue_meta.borrow_mut().insert(key.clone(), None);
            }
            let Some(details) = self.details_of(&key.0, &key.1) else { continue };
            self.tasks.spawn(async move {
                let Ok(meta) = details.await else { return };
                on_ui(move |library| {
                    let fresh = Some((meta.background.clone(), meta.videos.clone()));
                    let before = library.home_top.continue_meta.borrow_mut().insert(key.clone(), fresh.clone());
                    // Nothing new since last time: the cards stay as they are.
                    if before.as_ref() == Some(&fresh) {
                        return;
                    }
                    let picture_changed = before.flatten().map(|(bg, _)| bg) != Some(meta.background.clone());
                    // Forget a poster shown meanwhile: the wide picture is better.
                    for at in 0..library.home_top.continuing.row_count() {
                        if let Some(mut card) = library.home_top.continuing.row_data(at) {
                            if card.kind.as_str() == key.0 && card.id.as_str() == key.1 && meta.background.is_some() && picture_changed {
                                card.has_image = false;
                                library.home_top.continuing.set_row_data(at, card);
                            }
                        }
                    }
                    let watched = library.store.continue_watching(super::CONTINUE_LENGTH).unwrap_or_default();
                    library.fill_continue_cards(&watched);
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episodes_left_count_from_the_current_one() {
        let video = |s: u32, e: u32, day: &str| haven_titles::Video {
            id: format!("tt1:{s}:{e}"),
            title: None,
            season: Some(s),
            episode: Some(e),
            released: Some(day.to_owned()),
            thumbnail: None,
        };
        let videos = vec![
            video(0, 1, "2020-01-01"),
            video(1, 1, "2020-01-01"),
            video(1, 2, "2020-01-08"),
            video(1, 3, "2020-01-15"),
            video(2, 1, "2030-01-01"),
        ];
        let finished: std::collections::HashSet<String> = ["tt1:1:3".to_owned()].into();
        assert_eq!(episodes_left(&videos, "tt1:1:1", &finished, "2026-11-19"), 2);
        assert_eq!(episodes_left(&videos, "tt9:1:1", &finished, "2026-11-19"), 0);
        assert_eq!(episode_badge("tt1:3:6"), "S03E06");
        assert_eq!(episode_badge("tt1"), "");
    }
}

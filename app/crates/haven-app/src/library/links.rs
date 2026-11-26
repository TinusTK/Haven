//! Titles Haven knows by another id than the IMDb one (a media server's
//! own, say) linked to their IMDb id, which is how Trakt and Simkl know
//! them. With the link a film or episode watched on a service shows as
//! watched on the server's title, and one watched on the server's title
//! is sent to the services.

use std::collections::{HashMap, HashSet};

use haven_core::storage::Progress;
use haven_metadata::is_imdb_id;

use super::Library;

/// A title's IMDb id and, for a series, its episodes' ids against their
/// season and episode numbers.
pub(super) struct Link {
    pub imdb: String,
    pub episodes: HashMap<String, (u32, u32)>,
}

/// The IMDb form of an episode's id, as Haven keeps what services send.
pub(super) fn imdb_episode(imdb: &str, (season, episode): (u32, u32)) -> String {
    format!("{imdb}:{season}:{episode}")
}

impl Library {
    /// The link of a title not named by its IMDb id, from its kept
    /// details (or the id itself, for a server that gathers add-on titles).
    pub(super) fn imdb_link(&self, kind: &str, title_id: &str) -> Option<Link> {
        if is_imdb_id(title_id) {
            return None;
        }
        let detail = self.kept_details(kind, title_id);
        let imdb = detail
            .as_ref()
            .and_then(|detail| detail.imdb_id.clone())
            .filter(|imdb| is_imdb_id(imdb))
            .or_else(|| haven_servers::split_id(title_id).and_then(|(_, item)| haven_servers::imdb_in(item)))?;
        let episodes = detail
            .map(|detail| {
                detail
                    .videos
                    .into_iter()
                    .filter_map(|video| Some((video.id, (video.season?, video.episode?))))
                    .collect()
            })
            .unwrap_or_default();
        Some(Link { imdb, episodes })
    }

    /// A title and video as the services know them: the IMDb id, and
    /// for an episode `<imdb>:<season>:<episode>`. `None` when there is
    /// no IMDb id to go by (or an episode's numbers are not known).
    pub(super) fn service_ids(&self, kind: &str, title_id: &str, video_id: &str) -> Option<(String, String)> {
        if is_imdb_id(title_id) {
            return Some((title_id.to_owned(), video_id.to_owned()));
        }
        let link = self.imdb_link(kind, title_id)?;
        let video = if video_id.is_empty() {
            String::new()
        } else if kind != "series" {
            link.imdb.clone()
        } else {
            imdb_episode(&link.imdb, *link.episodes.get(video_id)?)
        };
        Some((link.imdb, video))
    }

    /// A title's films or episodes watched to the end: its own records,
    /// and those kept under its IMDb id (from a service) put in its ids.
    pub(super) fn finished_of(&self, kind: &str, title_id: &str) -> HashSet<String> {
        let mut done: HashSet<String> = self.store.finished_videos(kind, title_id).unwrap_or_default().into_iter().collect();
        let Some(link) = self.imdb_link(kind, title_id) else { return done };
        let elsewhere: HashSet<String> = self.store.finished_videos(kind, &link.imdb).unwrap_or_default().into_iter().collect();
        if kind == "series" {
            done.extend(
                link.episodes
                    .into_iter()
                    .filter(|(_, numbers)| elsewhere.contains(&imdb_episode(&link.imdb, *numbers)))
                    .map(|(id, _)| id),
            );
        } else if !elsewhere.is_empty() {
            done.insert(title_id.to_owned());
        }
        done
    }

    /// Takes away the record kept under the IMDb id of a video being
    /// marked unwatched on a title known by another id.
    pub(super) fn forget_linked(&self, kind: &str, title_id: &str, video_id: &str) {
        if is_imdb_id(title_id) {
            return;
        }
        let Some((imdb, video)) = self.service_ids(kind, title_id, video_id) else { return };
        if kind == "series" {
            let _ = self.store.remove_video_progress(kind, &imdb, &video);
        } else {
            for recorded in self.store.finished_videos(kind, &imdb).unwrap_or_default() {
                let _ = self.store.remove_video_progress(kind, &imdb, &recorded);
            }
        }
    }

    /// Takes the watched marks an account no longer has off this device:
    /// each `(kind, imdb, video)` under its IMDb id, and the same film or
    /// episode of a media server's title linked to it (so it is not sent
    /// back as watched). Partly watched videos keep their place. Says, for
    /// each, whether anything was taken off.
    pub(super) fn forget_watched(&self, gone: &[(String, String, String)]) -> Vec<bool> {
        if gone.is_empty() {
            return Vec::new();
        }
        // Server titles' finished videos, as the services know them.
        let mut links = HashMap::new();
        let linked: Vec<(Progress, String, String)> = self
            .store
            .all_progress()
            .unwrap_or_default()
            .into_iter()
            .filter(|row| row.finished && !is_imdb_id(&row.title_id))
            .filter_map(|row| {
                let link = links
                    .entry((row.kind.clone(), row.title_id.clone()))
                    .or_insert_with(|| self.imdb_link(&row.kind, &row.title_id))
                    .as_ref()?;
                let video = if row.kind == "series" { imdb_episode(&link.imdb, *link.episodes.get(&row.video_id)?) } else { link.imdb.clone() };
                let imdb = link.imdb.clone();
                Some((row, imdb, video))
            })
            .collect();
        gone.iter()
            .map(|(kind, imdb, video)| {
                let mut any = false;
                let own: Vec<String> = if kind == "series" {
                    vec![video.clone()]
                } else {
                    self.store.finished_videos(kind, imdb).unwrap_or_default()
                };
                for recorded in own {
                    if self.store.progress(kind, imdb, &recorded).ok().flatten().is_some_and(|row| row.finished) {
                        any |= self.store.remove_video_progress(kind, imdb, &recorded).is_ok();
                    }
                }
                for (row, linked_imdb, linked_video) in &linked {
                    if &row.kind == kind && linked_imdb == imdb && (kind != "series" || linked_video == video) {
                        any |= self.store.remove_video_progress(&row.kind, &row.title_id, &row.video_id).is_ok();
                    }
                }
                any
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::imdb_episode;

    #[test]
    fn episodes_are_named_as_services_send_them() {
        assert_eq!(imdb_episode("tt7631058", (1, 2)), "tt7631058:1:2");
    }
}

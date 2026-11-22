//! Films and series from the user's Xtream Codes accounts, offered as
//! places to watch a title beside the media servers and add-ons. An
//! account's lists are fetched the first time they are needed in a run
//! and kept in memory: they can be tens of thousands of titles long.

use std::{collections::HashMap, sync::Arc, time::Duration};

use haven_titles::Stream;
use haven_core::settings::SwitchedOff;
use haven_livetv::{
    OnDemand, XtreamAccount, on_demand_matches, xtream_episode, xtream_films, xtream_series,
};
use haven_servers::Wanted;

use super::{Library, on_ui};

/// An account's whole list of films can be large.
const LIST_BYTES: usize = 128 * 1024 * 1024;
const LIST_WAIT: Duration = Duration::from_secs(120);
const ANSWER_BYTES: usize = 8 * 1024 * 1024;
/// A provider may list a series several times; this many are looked into.
const SERIES_TRIED: usize = 3;

/// The lists already fetched this run: by account address, then films
/// (false) or series (true).
pub(super) type Lists = Arc<tokio::sync::Mutex<HashMap<(String, bool), Arc<Vec<OnDemand>>>>>;

/// Downloads an answer of limited size. Errors never carry the address,
/// which holds the account's password.
async fn fetch(http: &reqwest::Client, url: &str, limit: usize, wait: Duration) -> Result<Vec<u8>, String> {
    let address = haven_core::http::web_url(url).map_err(|_| "the account's address is not usable".to_owned())?;
    let failed = |err: reqwest::Error| {
        if err.is_timeout() { "the server took too long".to_owned() } else { "the server could not be reached".to_owned() }
    };
    let mut response = http.get(address).timeout(wait).send().await.map_err(failed)?;
    if !response.status().is_success() {
        return Err(format!("the server answered with error {}", response.status().as_u16()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(failed)? {
        if body.len() + chunk.len() > limit {
            return Err("the list was too long".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// The account's films or series, from memory or the network.
async fn listed(
    http: &reqwest::Client,
    lists: &Lists,
    account: &XtreamAccount,
    series: bool,
) -> Result<Arc<Vec<OnDemand>>, String> {
    let key = (account.source_url(), series);
    // Held while fetching, so two titles opened at once fetch it once.
    let mut lists = lists.lock().await;
    if let Some(known) = lists.get(&key) {
        return Ok(known.clone());
    }
    let action = if series { "get_series" } else { "get_vod_streams" };
    let bytes = fetch(http, &account.api(Some(action)), LIST_BYTES, LIST_WAIT).await?;
    let parsed = tokio::task::spawn_blocking(move || {
        let text = String::from_utf8_lossy(&bytes);
        if series { xtream_series(&text) } else { xtream_films(&text) }
    })
    .await
    .map_err(|_| "the list could not be read".to_owned())?
    .map_err(|err| err.to_string())?;
    let parsed = Arc::new(parsed);
    lists.insert(key, parsed.clone());
    Ok(parsed)
}

/// The ways to play the wanted film or episode from one account.
async fn streams(
    http: &reqwest::Client,
    lists: &Lists,
    account: &XtreamAccount,
    wanted: &Wanted,
) -> Result<Vec<Stream>, String> {
    let all = listed(http, lists, account, wanted.series).await?;
    let found: Vec<OnDemand> =
        on_demand_matches(&all, &wanted.name, wanted.year.as_deref(), wanted.tmdb.as_deref())
            .into_iter()
            .cloned()
            .collect();
    let file = |extension: &str| {
        Some(extension.to_uppercase()).filter(|ending| !ending.is_empty()).unwrap_or_else(|| "File".to_owned())
    };
    if !wanted.series {
        return Ok(found
            .iter()
            .map(|film| Stream {
                name: Some(film.name.clone()),
                description: Some(file(&film.extension)),
                url: Some(account.film(&film.id, &film.extension)),
                ..Stream::default()
            })
            .collect());
    }
    let Some((season, episode)) = wanted.episode else { return Ok(Vec::new()) };
    let mut streams = Vec::new();
    for show in found.iter().take(SERIES_TRIED) {
        let url = format!("{}&series_id={}", account.api(Some("get_series_info")), show.id);
        let Ok(info) = fetch(http, &url, ANSWER_BYTES, Duration::from_secs(30)).await else { continue };
        if let Some((id, extension)) = xtream_episode(&String::from_utf8_lossy(&info), season, episode) {
            streams.push(Stream {
                name: Some(format!("{}  S{season} E{episode}", show.name)),
                description: Some(file(&extension)),
                url: Some(account.episode(&id, &extension)),
                ..Stream::default()
            });
        }
    }
    Ok(streams)
}

impl Library {
    /// The Xtream Codes accounts that are switched on.
    fn accounts(&self) -> Vec<XtreamAccount> {
        let off = SwitchedOff::load(&self.store).unwrap_or_default().tv_sources;
        self.store
            .tv_sources()
            .unwrap_or_default()
            .iter()
            .filter(|source| !off.contains(&source.url))
            .filter_map(|source| XtreamAccount::from_source_url(&source.url))
            .collect()
    }

    /// Asks each Xtream Codes account whether its films or series hold
    /// the wanted title. Returns how many were asked; each answers
    /// through `streams_loaded` like a media server or an add-on.
    pub(super) fn ask_accounts(&self, generation: u64, wanted: &Wanted) -> usize {
        if wanted.name.trim().is_empty() || (wanted.series && wanted.episode.is_none()) {
            return 0;
        }
        let accounts = self.accounts();
        for account in &accounts {
            let (http, lists, account, wanted) =
                (self.http.clone(), self.on_demand.clone(), account.clone(), wanted.clone());
            self.tasks.spawn(async move {
                let found = streams(&http, &lists, &account, &wanted).await;
                let source = format!("Xtream · {}", account.label());
                on_ui(move |library| library.streams_loaded(generation, &source, None, found));
            });
        }
        accounts.len()
    }
}

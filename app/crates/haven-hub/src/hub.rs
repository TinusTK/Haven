//! The Hub's state: its database, its add-ons, the paired devices and
//! the pairing code.

use std::sync::Mutex;

use haven_addons::{Manifest, parse_manifest};
use haven_core::storage::Store;
use haven_titles::{Catalog, Catalogues, MetaDetail, MetaPreview, Source, Stream};
use ring::{digest, rand::SecureRandom};
use serde::{Deserialize, Serialize};

const DEVICES: &str = "hub_devices";

/// A device that paired with the Hub. Only a hash of its key is kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
    pub key_hash: String,
    pub paired_at: i64,
}

/// One add-on catalogue the Hub offers as a library.
#[derive(Debug, Clone)]
pub struct Offered {
    pub id: String,
    pub source: Source,
    pub catalog: Catalog,
}

pub struct Hub {
    pub store: Mutex<Store>,
    pub catalogues: Catalogues,
    pub addons: haven_addons::Client,
    /// The code a new device enters to pair; a new one after each pairing.
    pairing_code: Mutex<String>,
    pub name: String,
    pub http: reqwest::Client,
    pub background: crate::background::Background,
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    ring::rand::SystemRandom::new().fill(&mut bytes).expect("no randomness on this machine");
    bytes
}

fn new_code() -> String {
    let n = u32::from_le_bytes(random_bytes::<4>()) % 1_000_000;
    format!("{n:06}")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash(key: &str) -> String {
    hex(digest::digest(&digest::SHA256, key.as_bytes()).as_ref())
}

/// Compares in time that does not depend on where they differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Hub {
    pub fn new(store: Store, http: reqwest::Client, name: String) -> Self {
        Self {
            store: Mutex::new(store),
            catalogues: Catalogues::new(http.clone()),
            addons: haven_addons::Client::new(http.clone()),
            pairing_code: Mutex::new(new_code()),
            name,
            http,
            background: Default::default(),
        }
    }

    pub fn pairing_code(&self) -> String {
        self.pairing_code.lock().unwrap().clone()
    }

    pub fn devices(&self) -> Vec<Device> {
        self.store.lock().unwrap().get(DEVICES).ok().flatten().unwrap_or_default()
    }

    fn save_devices(&self, devices: &[Device]) {
        if let Err(err) = self.store.lock().unwrap().put(DEVICES, &devices) {
            eprintln!("haven-hub: could not save the paired devices: {err}");
        }
    }

    /// Pairs a device that gave the right code: hands back its key.
    pub fn pair(&self, code: &str, device: &str) -> Option<String> {
        let mut current = self.pairing_code.lock().unwrap();
        if !same(code.trim(), &current) {
            return None;
        }
        *current = new_code();
        drop(current);
        let key = hex(&random_bytes::<32>());
        let mut devices = self.devices();
        let name = device.trim();
        devices.push(Device {
            name: if name.is_empty() { "A device".to_owned() } else { name.chars().take(60).collect() },
            key_hash: hash(&key),
            paired_at: now_ms(),
        });
        self.save_devices(&devices);
        Some(key)
    }

    /// Whether `key` belongs to a paired device.
    pub fn knows(&self, key: &str) -> bool {
        let wanted = hash(key);
        self.devices().iter().any(|device| same(&device.key_hash, &wanted))
    }

    pub fn forget_device(&self, index: usize) {
        let mut devices = self.devices();
        if index < devices.len() {
            devices.remove(index);
            self.save_devices(&devices);
        }
    }

    // ---- Add-ons -------------------------------------------------------

    /// The installed add-ons with their manifests.
    pub fn installed(&self) -> Vec<(String, Manifest)> {
        let stored = self.store.lock().unwrap().addons().unwrap_or_default();
        stored
            .into_iter()
            .filter_map(|addon| parse_manifest(&addon.manifest).ok().map(|m| (addon.url, m)))
            .collect()
    }

    /// The add-ons switched on: the only ones the Hub uses.
    fn sources(&self) -> Vec<Source> {
        self.installed()
            .into_iter()
            .filter(|(url, _)| self.addon_on(url))
            .map(|(url, manifest)| manifest.source(&url))
            .collect()
    }

    pub async fn install(&self, address: &str) -> Result<String, String> {
        let url = haven_addons::manifest_url(address).map_err(|err| err.to_string())?;
        let json = self.addons.manifest_json(&url).await.map_err(|err| err.to_string())?;
        let manifest = parse_manifest(&json).map_err(|err| err.to_string())?;
        self.store.lock().unwrap().save_addon(&url, &json).map_err(|err| err.to_string())?;
        Ok(manifest.name)
    }

    pub fn remove(&self, url: &str) {
        let _ = self.store.lock().unwrap().remove_addon(url);
        self.switch_addon(url, true);
    }

    /// Fetches every add-on's manifest again, so new catalogues show.
    pub async fn refresh_addons(&self) {
        for (url, _) in self.installed() {
            if let Ok(json) = self.addons.manifest_json(&url).await {
                if parse_manifest(&json).is_ok() {
                    let _ = self.store.lock().unwrap().save_addon(&url, &json);
                }
            }
        }
    }

    // ---- What apps ask for --------------------------------------------

    /// Every catalogue that shows as a plain shelf, with a stable id.
    pub fn offered(&self) -> Vec<Offered> {
        self.sources()
            .into_iter()
            .flat_map(|source| {
                source
                    .catalogs
                    .iter()
                    .filter(|c| !c.needs_input() || c.browsable())
                    .map(|catalog| Offered {
                        id: hex(format!("{}|{}|{}", source.url, catalog.kind, catalog.id).as_bytes()),
                        source: source.clone(),
                        catalog: catalog.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub async fn catalog(
        &self,
        library: &str,
        genre: Option<&str>,
        skip: usize,
    ) -> Result<Vec<MetaPreview>, String> {
        let offered = self.offered().into_iter().find(|o| o.id == library).ok_or("no such library")?;
        // A catalogue that needs a genre gets its first one.
        let genre = genre.or_else(|| {
            offered.catalog.needs_input().then(|| offered.catalog.genres().first().map(String::as_str)).flatten()
        });
        self.catalogues
            .catalog_page(&offered.source.url, &offered.catalog, genre, skip)
            .await
            .map_err(|err| err.to_string())
    }

    pub async fn search(&self, words: &str) -> Vec<MetaPreview> {
        let mut found = Vec::new();
        for source in self.sources() {
            for catalog in source.catalogs.iter().filter(|c| c.searchable()) {
                if let Ok(titles) = self.catalogues.catalog(&source.url, catalog, Some(words)).await {
                    found.extend(titles);
                }
            }
        }
        haven_titles::without_repeats(found)
    }

    /// Details from the first add-on that knows the title, else from the
    /// free catalogue for IMDb ids.
    pub async fn meta(&self, kind: &str, id: &str) -> Result<MetaDetail, String> {
        let mut sources: Vec<Source> =
            self.sources().into_iter().filter(|s| s.provides("meta", kind, id)).collect();
        let builtin = Source::cinemeta(None);
        if builtin.provides("meta", kind, id) {
            sources.push(builtin);
        }
        let mut last = "nothing knows this title".to_owned();
        for source in sources {
            match self.catalogues.meta(&source.url, kind, id).await {
                Ok(meta) => return Ok(meta),
                Err(err) => last = err.to_string(),
            }
        }
        Err(last)
    }

    /// Every add-on's ways to watch a film or episode, as plain rows.
    pub async fn streams(&self, kind: &str, id: &str) -> Vec<Stream> {
        let mut all = Vec::new();
        for source in self.sources().into_iter().filter(|s| s.provides("stream", kind, id)) {
            if let Ok(streams) = self.addons.streams(&source.url, kind, id).await {
                all.extend(streams.into_iter().map(|mut stream| {
                    // Grouped under the Hub in the app, not by add-on.
                    stream.provider = None;
                    stream
                }));
            }
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_pairs_once_with_the_code_and_its_key_works() {
        let hub = Hub::new(Store::in_memory().unwrap(), haven_core::http::client().unwrap(), "Test".into());
        let code = hub.pairing_code();
        assert!(hub.pair("000000x", "Phone").is_none());
        let key = hub.pair(&code, "Phone").unwrap();
        assert!(hub.knows(&key));
        assert!(!hub.knows("not a key"));
        // The code changes once used.
        assert_ne!(hub.pairing_code(), code);
        assert!(hub.pair(&code, "Another").is_none() || hub.pairing_code() == code);
        // Only a hash is kept.
        assert!(hub.devices().iter().all(|d| d.key_hash != key));
        hub.forget_device(0);
        assert!(!hub.knows(&key));
    }
}

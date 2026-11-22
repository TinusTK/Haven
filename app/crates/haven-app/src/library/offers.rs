//! The services that carry a title (subscription, rental, purchase), as
//! TMDB passes them on from JustWatch, and the country they are shown for.

use haven_addons::MetaDetail;
use slint::{Color, ModelRc, SharedString, VecModel};

use super::{Library, on_ui};
use crate::{AppWindow, OfferRow};

const COUNTRY: &str = "country";
/// Looked in, in this order, when the chosen country has nothing.
const FALLBACKS: [&str; 4] = ["US", "GB", "CA", "AU"];

/// The countries offered in Settings: two-letter code and name.
const COUNTRIES: [(&str, &str); 40] = [
    ("AR", "Argentina"), ("AU", "Australia"), ("AT", "Austria"), ("BE", "Belgium"),
    ("BR", "Brazil"), ("CA", "Canada"), ("CL", "Chile"), ("CO", "Colombia"),
    ("CZ", "Czechia"), ("DK", "Denmark"), ("FI", "Finland"), ("FR", "France"),
    ("DE", "Germany"), ("GR", "Greece"), ("HK", "Hong Kong"), ("HU", "Hungary"),
    ("IN", "India"), ("ID", "Indonesia"), ("IE", "Ireland"), ("IL", "Israel"),
    ("IT", "Italy"), ("JP", "Japan"), ("MY", "Malaysia"), ("MX", "Mexico"),
    ("NL", "Netherlands"), ("NZ", "New Zealand"), ("NO", "Norway"), ("PH", "Philippines"),
    ("PL", "Poland"), ("PT", "Portugal"), ("SG", "Singapore"), ("ZA", "South Africa"),
    ("KR", "South Korea"), ("ES", "Spain"), ("SE", "Sweden"), ("CH", "Switzerland"),
    ("TH", "Thailand"), ("TR", "Türkiye"), ("GB", "United Kingdom"), ("US", "United States"),
];

/// Haven's own tile for a service: the letters on it, its background
/// and its ink. Our own designs (no company's logo), matched by name.
const TILES: [(&str, &str, u32, u32); 34] = [
    ("netflix", "N", 0x1a0709, 0xe5484d),
    ("amazon prime", "pv", 0x0b2236, 0x5ab0f0),
    ("prime video", "pv", 0x0b2236, 0x5ab0f0),
    ("amazon video", "a", 0x0b2236, 0x5ab0f0),
    ("amazon", "a", 0x0b2236, 0x5ab0f0),
    ("disney", "D+", 0x101a40, 0x8fb0ff),
    ("hbo max", "max", 0x1c1030, 0xb993ff),
    ("max", "max", 0x1c1030, 0xb993ff),
    ("apple tv", "tv", 0x1d1f24, 0xe9ebef),
    ("apple itunes", "tv", 0x1d1f24, 0xe9ebef),
    ("google play", "G", 0x0d2418, 0x5fd18f),
    ("youtube", "YT", 0x240808, 0xff5a5a),
    ("stan", "S", 0x0a1a33, 0x4a9cff),
    ("binge", "B", 0x21082a, 0xe86bf5),
    ("foxtel", "F", 0x2a1206, 0xff8a3d),
    ("kayo", "K", 0x062a20, 0x45e0b0),
    ("paramount", "P+", 0x081a3a, 0x5c8dff),
    ("hulu", "h", 0x062612, 0x3ee07a),
    ("peacock", "pk", 0x141414, 0xf3c94b),
    ("tubi", "tb", 0x2a1a05, 0xffb347),
    ("pluto", "pl", 0x1f1a05, 0xf5e04a),
    ("sbs", "SBS", 0x1e1a05, 0xf7d24a),
    ("abc iview", "iv", 0x062224, 0x46d6dc),
    ("7plus", "7+", 0x260808, 0xff6b6b),
    ("9now", "9N", 0x07173a, 0x6aa2ff),
    ("10 play", "10", 0x1a1a1a, 0xf0f0f0),
    ("bbc iplayer", "iP", 0x1f0a26, 0xe07bff),
    ("itvx", "X", 0x08232a, 0x4fd2e8),
    ("channel 4", "4", 0x14141f, 0xb8b8ff),
    ("now tv", "NOW", 0x0b2a1f, 0x3fe0a0),
    ("sky", "sky", 0x0a1630, 0x6a9cff),
    ("crunchyroll", "CR", 0x2a1405, 0xff9a3d),
    ("mubi", "M", 0x161616, 0xeeeeee),
    ("microsoft", "MS", 0x0f1a24, 0x7cc4ff),
];

/// The tile for a service name.
fn tile_of(name: &str) -> (String, Color, Color) {
    let lower = name.to_lowercase();
    let rgb = |hex: u32| Color::from_rgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    match TILES.iter().find(|(key, ..)| lower.contains(key)) {
        Some((_, letters, bg, ink)) => ((*letters).to_owned(), rgb(*bg), rgb(*ink)),
        // Any other service: its first letter in Haven's gold.
        None => (
            name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default(),
            rgb(0x1b1d24),
            rgb(0xf0c36a),
        ),
    }
}

/// The four groups shown, and the ways of watching each one holds.
const GROUPS: [(&str, &[&str]); 4] = [
    ("Stream", &["Subscription"]),
    ("Free", &["Free", "Free with ads"]),
    ("Rent", &["Rent"]),
    ("Buy", &["Buy"]),
];

/// JustWatch's search for a title in a country (JustWatch writes the
/// United Kingdom as "uk").
fn justwatch_link(country: &str, title: &str) -> String {
    let country = if country.eq_ignore_ascii_case("GB") { "uk".to_owned() } else { country.to_lowercase() };
    let query: String = title
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    format!("https://www.justwatch.com/{country}/search?q={query}")
}

/// One tile per service per group, groups in order; the first tile of
/// each group carries the group's name.
fn offer_rows(services: &[haven_metadata::Service]) -> Vec<OfferRow> {
    let mut rows = Vec::new();
    for (group, ways) in GROUPS {
        let mut first = true;
        for service in services.iter().filter(|service| service.ways.iter().any(|way| ways.contains(way))) {
            let (letters, tint, ink) = tile_of(&service.name);
            rows.push(OfferRow {
                name: service.name.as_str().into(),
                ways: service.ways.join(", ").into(),
                group: if first { group.into() } else { "".into() },
                letters: letters.into(),
                tint,
                ink,
            });
            first = false;
        }
    }
    rows
}

fn country_name(code: &str) -> &str {
    COUNTRIES.iter().find(|(known, _)| *known == code).map_or(code, |(_, name)| name)
}

/// The region part of a locale such as "en-AU", "en_AU.UTF-8" or
/// "zh-Hant-HK", if it is one of the countries offered.
fn region_of(locale: &str) -> Option<String> {
    let locale = locale.split(['.', '@']).next()?;
    locale
        .split(['_', '-'])
        .skip(1)
        .map(str::to_uppercase)
        .find(|part| part.len() == 2 && COUNTRIES.iter().any(|(code, _)| code == part))
}

/// The country this device is set to: the system's own region and
/// language settings (macOS and iOS, Windows, Android), then the usual
/// environment settings (Linux); the United States if none says.
fn device_country() -> String {
    sys_locale::get_locales()
        .chain(["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|name| std::env::var(name).ok()))
        .find_map(|locale| region_of(&locale))
        .unwrap_or_else(|| "US".to_owned())
}

impl Library {
    /// The country the user picked, if they did.
    fn chosen_country(&self) -> Option<String> {
        self.store
            .get::<String>(COUNTRY)
            .ok()
            .flatten()
            .filter(|code| COUNTRIES.iter().any(|(known, _)| known == code))
    }

    /// The country in use: the user's pick, else the device's own
    /// (looked at each time, so it follows the device until one is picked).
    fn country(&self) -> String {
        self.chosen_country().unwrap_or_else(device_country)
    }

    /// The list in Settings: "Automatic (the device's country)" first,
    /// then every country; and which line is chosen.
    fn show_countries(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let detected = device_country();
        let mut names: Vec<SharedString> =
            vec![format!("Automatic ({})", country_name(&detected)).into()];
        names.extend(COUNTRIES.iter().map(|(_, name)| SharedString::from(*name)));
        ui.set_countries(ModelRc::new(VecModel::from(names)));
        let index = match self.chosen_country() {
            Some(code) => COUNTRIES.iter().position(|(known, _)| *known == code).map_or(0, |at| at + 1),
            None => 0,
        };
        ui.set_country_index(index as i32);
    }

    pub(super) fn start_offers(self: &std::rc::Rc<Self>, ui: &AppWindow) {
        self.show_countries();
        ui.on_country_selected({
            let library = self.clone();
            move |index| {
                // Line 0 is Automatic: forget the pick and follow the device.
                let saved = match index {
                    0 => library.store.remove(COUNTRY),
                    _ => match COUNTRIES.get(index.max(1) as usize - 1) {
                        Some((code, _)) => library.store.put(COUNTRY, &(*code).to_owned()),
                        None => Ok(()),
                    },
                };
                if let Err(err) = saved {
                    eprintln!("haven: could not save the country: {err}");
                }
                library.show_countries();
            }
        });
    }

    pub(super) fn clear_offers(&self, note: &str) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_offers(ModelRc::default());
            ui.set_offers_note(note.into());
            ui.set_offers_link("".into());
        }
    }

    /// Looks up which services carry the open title.
    pub(super) fn load_offers(&self, generation: u64, meta: &MetaDetail) {
        let Some(tmdb) = self.tmdb() else {
            return self.clear_offers(
                "Add a TMDB key in Settings to see which streaming services, rental and download shops have this.",
            );
        };
        self.clear_offers("Looking for services that have this…");
        let series = meta.kind == "series";
        let number = meta
            .tmdb_id
            .as_deref()
            .and_then(|number| number.parse::<u64>().ok())
            .or_else(|| haven_metadata::split_id(&meta.id).map(|(_, number)| number));
        let imdb = meta
            .imdb_id
            .clone()
            .or_else(|| Some(meta.id.clone()))
            .filter(|id| haven_metadata::is_imdb_id(id));
        let chosen = self.country();
        let title = meta.name.clone();
        let mut countries = vec![chosen.clone()];
        countries.extend(FALLBACKS.iter().map(|code| (*code).to_owned()).filter(|code| *code != chosen));
        self.tasks.spawn(async move {
            let found = async {
                let number = match (number, imdb) {
                    (Some(number), _) => Some(number),
                    (None, Some(imdb)) => tmdb.number_of(series, &imdb).await?,
                    (None, None) => None,
                };
                match number {
                    Some(number) => tmdb.offers(series, number, &countries).await,
                    None => Ok(None),
                }
            }
            .await;
            on_ui(move |library| {
                if generation != library.detail_generation.get() {
                    return;
                }
                let Some(ui) = library.ui.upgrade() else { return };
                match found {
                    Ok(Some(offers)) => {
                        let rows = offer_rows(&offers.services);
                        ui.set_offers(ModelRc::new(VecModel::from(rows)));
                        ui.set_offers_link(justwatch_link(&offers.country, &title).into());
                        ui.set_offers_note(
                            if offers.country == chosen {
                                country_name(&offers.country).to_owned()
                            } else {
                                format!("{} (nothing in {})", country_name(&offers.country), country_name(&chosen))
                            }
                            .into(),
                        );
                    }
                    Ok(None) => library.clear_offers(&format!(
                        "No service is known to have this in {} or the other countries Haven looks in.",
                        country_name(&chosen)
                    )),
                    Err(err) => library.clear_offers(&format!("Could not look this up: {err}.")),
                }
            });
        });
    }
}

#[cfg(test)]
mod country_tests {
    use super::region_of;

    #[test]
    fn the_region_comes_from_any_way_of_writing_a_locale() {
        assert_eq!(region_of("en-AU").as_deref(), Some("AU"));
        assert_eq!(region_of("en_GB.UTF-8").as_deref(), Some("GB"));
        assert_eq!(region_of("zh-Hant-HK").as_deref(), Some("HK"));
        assert_eq!(region_of("en"), None);
        assert_eq!(region_of("C.UTF-8"), None);
    }
}

#[cfg(test)]
mod offer_tests {
    use super::*;

    #[test]
    fn offers_fall_into_their_groups_with_our_tiles() {
        let services = vec![
            haven_metadata::Service { name: "Netflix".into(), ways: vec!["Subscription"] },
            haven_metadata::Service { name: "Apple TV".into(), ways: vec!["Rent", "Buy"] },
            haven_metadata::Service { name: "Tubi TV".into(), ways: vec!["Free with ads"] },
            haven_metadata::Service { name: "Some Shop".into(), ways: vec!["Buy"] },
        ];
        let rows = offer_rows(&services);
        let seen: Vec<(String, String)> = rows.iter().map(|r| (r.group.to_string(), r.name.to_string())).collect();
        assert_eq!(
            seen,
            vec![
                ("Stream".into(), "Netflix".into()),
                ("Free".into(), "Tubi TV".into()),
                ("Rent".into(), "Apple TV".into()),
                ("Buy".into(), "Apple TV".into()),
                ("".into(), "Some Shop".into()),
            ]
        );
        assert_eq!(rows[0].letters, "N");
        assert_eq!(rows[4].letters, "S");
    }

    #[test]
    fn justwatch_links_use_its_country_names() {
        assert_eq!(justwatch_link("GB", "Dune: Part Two"), "https://www.justwatch.com/uk/search?q=Dune%3A%20Part%20Two");
        assert_eq!(justwatch_link("AU", "Up"), "https://www.justwatch.com/au/search?q=Up");
    }
}

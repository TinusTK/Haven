//! The services that carry a title (subscription, rental, purchase), as
//! TMDB passes them on from JustWatch, and the country they are shown for.

use haven_addons::MetaDetail;
use slint::{ModelRc, SharedString, VecModel};

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
                        let rows: Vec<OfferRow> = offers
                            .services
                            .iter()
                            .map(|service| OfferRow {
                                name: service.name.as_str().into(),
                                ways: service.ways.join(", ").into(),
                            })
                            .collect();
                        ui.set_offers(ModelRc::new(VecModel::from(rows)));
                        ui.set_offers_link(offers.link.unwrap_or_default().into());
                        ui.set_offers_note(
                            if offers.country == chosen {
                                format!("In {}.", country_name(&offers.country))
                            } else {
                                format!(
                                    "Nothing in {}. These are in {}.",
                                    country_name(&chosen),
                                    country_name(&offers.country)
                                )
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

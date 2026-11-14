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

/// The country the computer is set to, from the usual environment
/// settings ("en_AU.UTF-8" gives AU); the United States if they do not say.
fn computer_country() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find_map(|locale| {
            let region = locale.split('.').next()?.split(['_', '-']).nth(1)?.to_uppercase();
            COUNTRIES.iter().any(|(code, _)| *code == region).then_some(region)
        })
        .unwrap_or_else(|| "US".to_owned())
}

impl Library {
    /// The chosen country's code.
    fn country(&self) -> String {
        self.store
            .get::<String>(COUNTRY)
            .ok()
            .flatten()
            .filter(|code| COUNTRIES.iter().any(|(known, _)| known == code))
            .unwrap_or_else(computer_country)
    }

    pub(super) fn start_offers(self: &std::rc::Rc<Self>, ui: &AppWindow) {
        let names: Vec<SharedString> = COUNTRIES.iter().map(|(_, name)| (*name).into()).collect();
        ui.set_countries(ModelRc::new(VecModel::from(names)));
        let country = self.country();
        ui.set_country_index(
            COUNTRIES.iter().position(|(code, _)| *code == country).unwrap_or(0) as i32,
        );
        ui.on_country_selected({
            let library = self.clone();
            move |index| {
                if let Some((code, _)) = COUNTRIES.get(index.max(0) as usize) {
                    if let Err(err) = library.store.put(COUNTRY, &(*code).to_owned()) {
                        eprintln!("haven: could not save the country: {err}");
                    }
                }
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

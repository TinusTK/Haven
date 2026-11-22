use serde::Deserialize;

use crate::{Error, Result};

/// One browsable list a catalogue service offers, such as "Popular" films.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Catalog {
    /// Content type: `movie`, `series`, `channel`, `tv`…
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub extra: Vec<Extra>,
    /// Older way of naming the extras a catalogue accepts.
    #[serde(default, rename = "extraSupported")]
    pub extra_supported: Vec<String>,
    /// Older way of naming required extras.
    #[serde(default, rename = "extraRequired")]
    pub extra_required: Vec<String>,
}

/// An option a catalogue accepts, such as `search`, `genre` or `skip`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Extra {
    pub name: String,
    #[serde(default, rename = "isRequired")]
    pub is_required: bool,
    /// The values it takes, such as the genres to choose from.
    #[serde(default, deserialize_with = "crate::lenient::texts")]
    pub options: Vec<String>,
}

impl Catalog {
    fn required(&self) -> impl Iterator<Item = &str> {
        self.extra
            .iter()
            .filter(|extra| extra.is_required)
            .map(|extra| extra.name.as_str())
            .chain(self.extra_required.iter().map(String::as_str))
    }

    /// True when the catalogue only answers with extra input (a search
    /// term, a genre…), so it cannot be shown as a plain shelf.
    pub fn needs_input(&self) -> bool {
        self.required().next().is_some()
    }

    /// True when the catalogue can be searched with nothing but a search
    /// term.
    pub fn searchable(&self) -> bool {
        let supports = self.extra.iter().any(|extra| extra.name == "search")
            || self.extra_supported.iter().any(|name| name == "search");
        supports && self.required().all(|name| name == "search")
    }

    /// The genres this catalogue can be narrowed to; empty if it has none.
    pub fn genres(&self) -> &[String] {
        self.extra.iter().find(|extra| extra.name == "genre").map_or(&[], |extra| &extra.options)
    }

    /// True when the catalogue hands out further pages (`skip`).
    pub fn pages(&self) -> bool {
        self.extra.iter().any(|extra| extra.name == "skip")
            || self.extra_supported.iter().any(|name| name == "skip")
    }

    /// True when the catalogue can be browsed with at most a genre chosen.
    pub fn browsable(&self) -> bool {
        self.required().all(|name| name == "genre" && !self.genres().is_empty())
    }

    /// What this catalogue's content is called: "films", "series"…
    pub fn kind_label(&self) -> &str {
        match self.kind.as_str() {
            "movie" => "films",
            "series" => "series",
            "channel" => "channels",
            "tv" => "TV",
            other => other,
        }
    }

    /// A shelf title such as "Popular films".
    pub fn title(&self) -> String {
        let kind = self.kind_label();
        match self.name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => format!("{name} {kind}"),
            _ => capitalised(kind),
        }
    }

    /// A title for this catalogue's search results, such as "Films".
    pub fn results_title(&self) -> String {
        capitalised(self.kind_label())
    }
}

fn capitalised(text: &str) -> String {
    let mut text = text.to_owned();
    if let Some(first) = text.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    text
}

/// Reads the catalogues a service lists in its description
/// (`{"catalogs": [...]}`); other fields are ignored.
pub fn parse_catalogs(json: &str) -> Result<Vec<Catalog>> {
    #[derive(Deserialize)]
    struct Described {
        #[serde(default)]
        catalogs: Vec<Catalog>,
    }
    let described: Described = serde_json::from_str(json).map_err(Error::BadResponse)?;
    Ok(described.catalogs)
}

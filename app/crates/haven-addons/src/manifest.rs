use serde::{Deserialize, Deserializer};

use crate::{Error, Result};

/// What an add-on says about itself. Fields Haven does not use are
/// ignored, so the raw JSON is what gets stored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub catalogs: Vec<Catalog>,
    /// What the add-on can be asked for: `catalog`, `meta`, `stream`…
    #[serde(default)]
    pub resources: Vec<Resource>,
    /// If set, the add-on only knows titles whose id starts with one of these.
    #[serde(default, rename = "idPrefixes")]
    pub id_prefixes: Option<Vec<String>>,
}

/// One kind of answer an add-on gives. The short form (just a name) uses
/// the manifest's own types and id prefixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub name: String,
    pub types: Option<Vec<String>>,
    pub id_prefixes: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for Resource {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Form {
            Short(String),
            Full {
                name: String,
                #[serde(default)]
                types: Option<Vec<String>>,
                #[serde(default, rename = "idPrefixes")]
                id_prefixes: Option<Vec<String>>,
            },
        }
        Ok(match Form::deserialize(de)? {
            Form::Short(name) => Self { name, types: None, id_prefixes: None },
            Form::Full { name, types, id_prefixes } => Self { name, types, id_prefixes },
        })
    }
}

/// One browsable list an add-on offers, such as "Popular" films.
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

impl Manifest {
    /// Whether this add-on can be asked for `resource` (`meta`, `stream`…)
    /// about the title `id` of type `kind`.
    pub fn provides(&self, resource: &str, kind: &str, id: &str) -> bool {
        let Some(resource) = self.resources.iter().find(|r| r.name == resource) else {
            return false;
        };
        let types = resource.types.as_ref().unwrap_or(&self.types);
        if !types.iter().any(|t| t == kind) {
            return false;
        }
        match resource.id_prefixes.as_ref().or(self.id_prefixes.as_ref()) {
            Some(prefixes) => prefixes.iter().any(|prefix| id.starts_with(prefix.as_str())),
            None => true,
        }
    }
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

pub fn parse_manifest(json: &str) -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_str(json).map_err(Error::BadManifest)?;
    if manifest.id.trim().is_empty() {
        return Err(Error::IncompleteManifest("id"));
    }
    if manifest.name.trim().is_empty() {
        return Err(Error::IncompleteManifest("name"));
    }
    Ok(manifest)
}

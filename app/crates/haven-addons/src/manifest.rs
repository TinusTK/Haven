use serde::{Deserialize, Deserializer};

use haven_titles::{Catalog, Serves, Source};

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

impl Manifest {
    /// This add-on as a source of catalogues, details and streams.
    pub fn source(&self, url: &str) -> Source {
        Source {
            url: url.to_owned(),
            name: self.name.clone(),
            catalogs: self.catalogs.clone(),
            serves: self
                .resources
                .iter()
                .map(|resource| Serves {
                    resource: resource.name.clone(),
                    types: resource.types.clone().unwrap_or_else(|| self.types.clone()),
                    id_prefixes: resource.id_prefixes.clone().or_else(|| self.id_prefixes.clone()),
                })
                .collect(),
        }
    }
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

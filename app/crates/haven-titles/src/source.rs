//! A source of catalogues and details: Haven's built-in catalogue, or
//! (in builds that have them) others the user chose.

use crate::Catalog;

/// Cinemeta: the free catalogue of films and series built into Haven. It
/// needs no key, so Home and Explore have something in them from the start.
pub const CINEMETA_URL: &str = "https://v3-cinemeta.strem.io/manifest.json";
pub const CINEMETA_NAME: &str = "Cinemeta";

/// One kind of answer a source gives, for which types and ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serves {
    /// `catalog`, `meta`, `stream`…
    pub resource: String,
    pub types: Vec<String>,
    /// If set, only ids that start with one of these.
    pub id_prefixes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Its description's address; everything else it serves sits next to it.
    pub url: String,
    pub name: String,
    pub catalogs: Vec<Catalog>,
    pub serves: Vec<Serves>,
}

impl Source {
    /// Whether this source can be asked for `resource` about the title `id`
    /// of type `kind`.
    pub fn provides(&self, resource: &str, kind: &str, id: &str) -> bool {
        self.serves.iter().any(|serves| {
            serves.resource == resource
                && serves.types.iter().any(|t| t == kind)
                && serves
                    .id_prefixes
                    .as_ref()
                    .is_none_or(|prefixes| prefixes.iter().any(|p| id.starts_with(p.as_str())))
        })
    }

    /// The built-in catalogue, with the catalogues it lists (from its own
    /// description, or the plain list below when that is not known yet).
    pub fn cinemeta(catalogs: Option<Vec<Catalog>>) -> Self {
        let types = vec!["movie".to_owned(), "series".to_owned()];
        let prefixes = Some(vec!["tt".to_owned()]);
        Self {
            url: CINEMETA_URL.to_owned(),
            name: CINEMETA_NAME.to_owned(),
            catalogs: catalogs.unwrap_or_else(cinemeta_fallback),
            serves: ["catalog", "meta"]
                .into_iter()
                .map(|resource| Serves {
                    resource: resource.to_owned(),
                    types: types.clone(),
                    id_prefixes: prefixes.clone(),
                })
                .collect(),
        }
    }
}

/// Popular and Featured films and series, searchable, with pages.
fn cinemeta_fallback() -> Vec<Catalog> {
    let mut list = Vec::new();
    for (id, name) in [("top", "Popular"), ("imdbRating", "Featured")] {
        for kind in ["movie", "series"] {
            let mut supported = vec!["skip".to_owned()];
            if id == "top" {
                supported.push("search".to_owned());
            }
            list.push(Catalog {
                kind: kind.to_owned(),
                id: id.to_owned(),
                name: Some(name.to_owned()),
                extra: Vec::new(),
                extra_supported: supported,
                extra_required: Vec::new(),
            });
        }
    }
    // Popular films and series first, then Featured, as Cinemeta lists them.
    list.sort_by_key(|catalog| catalog.id != "top");
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cinemeta_gives_details_for_imdb_ids_only() {
        let source = Source::cinemeta(None);
        assert!(source.provides("meta", "movie", "tt0111161"));
        assert!(source.provides("meta", "series", "tt0903747"));
        assert!(!source.provides("meta", "movie", "tmdb:603"));
        assert!(!source.provides("stream", "movie", "tt0111161"));
        assert_eq!(source.catalogs.len(), 4);
        assert!(source.catalogs[0].searchable());
        assert!(source.catalogs.iter().all(|c| !c.needs_input()));
    }
}

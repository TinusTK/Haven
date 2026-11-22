use url::Url;

use crate::{Catalog, Error, Result};

fn web_url(address: &str) -> Result<Url> {
    let url = Url::parse(address.trim()).map_err(|_| Error::InvalidAddress(address.trim().to_owned()))?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(Error::InvalidAddress(address.trim().to_owned())),
    }
}

/// Percent-encodes one part of a path the way catalogue services expect: everything
/// except letters, digits and `-_.~`.
fn encoded(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for byte in part.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The address of something a service serves next to its description
/// (`…/manifest.json`):
/// `<resource>/<kind>/<id>.json`, or with an extra option
/// `<resource>/<kind>/<id>/<name>=<value>.json`.
pub fn resource_url(
    description_url: &str,
    resource: &str,
    kind: &str,
    id: &str,
    extra: Option<(&str, &str)>,
) -> Result<String> {
    resource_url_with(description_url, resource, kind, id, extra.as_slice())
}

/// As [`resource_url`], with any number of extra options
/// (`…/<id>/<name>=<value>&<name>=<value>.json`).
pub fn resource_url_with(
    description_url: &str,
    resource: &str,
    kind: &str,
    id: &str,
    extras: &[(&str, &str)],
) -> Result<String> {
    let invalid = || Error::InvalidAddress(description_url.to_owned());
    let mut url = web_url(description_url)?;
    let folder = url.path().strip_suffix("manifest.json").ok_or_else(invalid)?;
    let mut path = format!("{folder}{}/{}/{}", encoded(resource), encoded(kind), encoded(id));
    for (at, (name, value)) in extras.iter().enumerate() {
        path.push(if at == 0 { '/' } else { '&' });
        path.push_str(&format!("{}={}", encoded(name), encoded(value)));
    }
    path.push_str(".json");
    url.set_path(&path);
    Ok(url.into())
}

/// The address of one of the service's catalogues, optionally searched.
pub fn catalog_url(description_url: &str, catalog: &Catalog, search: Option<&str>) -> Result<String> {
    resource_url(
        description_url,
        "catalog",
        &catalog.kind,
        &catalog.id,
        search.map(|query| ("search", query)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(kind: &str, id: &str) -> Catalog {
        Catalog {
            kind: kind.into(),
            id: id.into(),
            name: None,
            extra: Vec::new(),
            extra_supported: Vec::new(),
            extra_required: Vec::new(),
        }
    }

    #[test]
    fn catalogues_sit_next_to_the_manifest() {
        assert_eq!(
            catalog_url("https://service.example/manifest.json", &catalog("movie", "top"), None)
                .unwrap(),
            "https://service.example/catalog/movie/top.json"
        );
        assert_eq!(
            catalog_url(
                "https://service.example/cfg=a%7Cb/manifest.json",
                &catalog("series", "new shows/1"),
                None
            )
            .unwrap(),
            "https://service.example/cfg=a%7Cb/catalog/series/new%20shows%2F1.json"
        );
    }

    #[test]
    fn search_terms_are_fully_encoded() {
        assert_eq!(
            catalog_url(
                "https://service.example/manifest.json",
                &catalog("movie", "top"),
                Some("tom & jerry: 100% café?")
            )
            .unwrap(),
            "https://service.example/catalog/movie/top/search=tom%20%26%20jerry%3A%20100%25%20caf%C3%A9%3F.json"
        );
    }

    #[test]
    fn details_and_streams_use_the_title_id() {
        let manifest = "https://service.example/manifest.json";
        assert_eq!(
            resource_url(manifest, "meta", "series", "tt0000004", None).unwrap(),
            "https://service.example/meta/series/tt0000004.json"
        );
        assert_eq!(
            resource_url(manifest, "stream", "series", "tt0000004:1:2", None).unwrap(),
            "https://service.example/stream/series/tt0000004%3A1%3A2.json"
        );
    }
}

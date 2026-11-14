use haven_core::http::web_url;

use crate::{Catalog, Error, Result};

/// Turns what the user typed or pasted into the add-on's manifest address.
///
/// Accepts a full `…/manifest.json` address, the add-on's base address, or
/// a `stremio://` install link.
pub fn manifest_url(input: &str) -> Result<String> {
    let input = input.trim();
    let invalid = || Error::InvalidAddress(input.to_owned());
    if input.is_empty() {
        return Err(invalid());
    }
    let address = match input.strip_prefix("stremio://") {
        Some(rest) => format!("https://{rest}"),
        None => input.to_owned(),
    };
    let mut url = web_url(&address).map_err(|_| invalid())?;
    if url.host_str().is_none_or(str::is_empty) {
        return Err(invalid());
    }
    url.set_fragment(None);
    if !url.path().ends_with("/manifest.json") {
        let path = format!("{}/manifest.json", url.path().trim_end_matches('/'));
        url.set_path(&path);
    }
    Ok(url.into())
}

/// Percent-encodes one part of a path the way add-ons expect: everything
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

/// The address of something an add-on serves next to its manifest:
/// `<resource>/<kind>/<id>.json`, or with an extra option
/// `<resource>/<kind>/<id>/<name>=<value>.json`.
pub fn resource_url(
    manifest_url: &str,
    resource: &str,
    kind: &str,
    id: &str,
    extra: Option<(&str, &str)>,
) -> Result<String> {
    resource_url_with(manifest_url, resource, kind, id, extra.as_slice())
}

/// As [`resource_url`], with any number of extra options
/// (`…/<id>/<name>=<value>&<name>=<value>.json`).
pub fn resource_url_with(
    manifest_url: &str,
    resource: &str,
    kind: &str,
    id: &str,
    extras: &[(&str, &str)],
) -> Result<String> {
    let invalid = || Error::InvalidAddress(manifest_url.to_owned());
    let mut url = web_url(manifest_url).map_err(|_| invalid())?;
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

/// The address of one of the add-on's catalogues, optionally searched.
pub fn catalog_url(manifest_url: &str, catalog: &Catalog, search: Option<&str>) -> Result<String> {
    resource_url(
        manifest_url,
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
    fn accepts_the_ways_people_paste_an_address() {
        let want = "https://addon.example/manifest.json";
        for input in [
            "https://addon.example/manifest.json",
            "  https://addon.example  ",
            "https://addon.example/",
            "stremio://addon.example/manifest.json",
            "https://addon.example/manifest.json#top",
        ] {
            assert_eq!(manifest_url(input).unwrap(), want, "{input}");
        }
        assert_eq!(
            manifest_url("http://127.0.0.1:7000/cfg=a%7Cb").unwrap(),
            "http://127.0.0.1:7000/cfg=a%7Cb/manifest.json"
        );
    }

    #[test]
    fn refuses_things_that_are_not_web_addresses() {
        for input in ["", "   ", "addon", "ftp://addon.example", "file:///tmp/manifest.json"] {
            assert!(matches!(manifest_url(input), Err(Error::InvalidAddress(_))), "{input}");
        }
    }

    #[test]
    fn catalogues_sit_next_to_the_manifest() {
        assert_eq!(
            catalog_url("https://addon.example/manifest.json", &catalog("movie", "top"), None)
                .unwrap(),
            "https://addon.example/catalog/movie/top.json"
        );
        assert_eq!(
            catalog_url(
                "https://addon.example/cfg=a%7Cb/manifest.json",
                &catalog("series", "new shows/1"),
                None
            )
            .unwrap(),
            "https://addon.example/cfg=a%7Cb/catalog/series/new%20shows%2F1.json"
        );
    }

    #[test]
    fn search_terms_are_fully_encoded() {
        assert_eq!(
            catalog_url(
                "https://addon.example/manifest.json",
                &catalog("movie", "top"),
                Some("tom & jerry: 100% café?")
            )
            .unwrap(),
            "https://addon.example/catalog/movie/top/search=tom%20%26%20jerry%3A%20100%25%20caf%C3%A9%3F.json"
        );
    }

    #[test]
    fn details_and_streams_use_the_title_id() {
        let manifest = "https://addon.example/manifest.json";
        assert_eq!(
            resource_url(manifest, "meta", "series", "tt0000004", None).unwrap(),
            "https://addon.example/meta/series/tt0000004.json"
        );
        assert_eq!(
            resource_url(manifest, "stream", "series", "tt0000004:1:2", None).unwrap(),
            "https://addon.example/stream/series/tt0000004%3A1%3A2.json"
        );
    }
}

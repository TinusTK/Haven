use haven_core::http::web_url;

use crate::{Error, Result};

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

#[cfg(test)]
mod tests {
    use super::*;

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

}

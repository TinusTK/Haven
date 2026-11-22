//! Parsing checked against made-up answers in the shape real add-ons
//! serve (extras in both the current and the older style, a year sent as
//! a number, entries with missing fields).

use haven_addons::{
    Error, StreamKind, parse_catalog, parse_manifest, parse_meta, parse_streams,
};

const MANIFEST: &str = include_str!("fixtures/manifest.json");
const CATALOG: &str = include_str!("fixtures/catalog.json");
const META: &str = include_str!("fixtures/meta.json");
const STREAMS: &str = include_str!("fixtures/streams.json");

#[test]
fn reads_a_manifest() {
    let manifest = parse_manifest(MANIFEST).unwrap();
    assert_eq!(manifest.id, "org.example.sample");
    assert_eq!(manifest.name, "Sample Catalogue");
    assert_eq!(manifest.version, "1.2.0");
    assert_eq!(manifest.types, ["movie", "series"]);
    assert_eq!(manifest.catalogs.len(), 5);

    let top = &manifest.catalogs[0];
    assert_eq!((top.kind.as_str(), top.id.as_str()), ("movie", "top"));
    assert_eq!(top.title(), "Popular films");
    assert!(!top.needs_input(), "optional extras do not block a shelf");
    assert_eq!(manifest.catalogs[4].title(), "Channels");
}

#[test]
fn catalogues_that_need_a_genre_or_ids_are_not_shelves() {
    let manifest = parse_manifest(MANIFEST).unwrap();
    let shelves: Vec<_> = manifest
        .catalogs
        .iter()
        .filter(|catalog| !catalog.needs_input())
        .map(|catalog| catalog.id.as_str())
        .collect();
    assert_eq!(shelves, ["top", "top", "picks"]);
}

#[test]
fn reads_a_catalogue_and_skips_broken_entries() {
    let metas = parse_catalog(CATALOG).unwrap();
    assert_eq!(metas.len(), 4, "entries with no id, or that are not objects, are left out");
    assert_eq!(metas[0].id, "tt0000001");
    assert_eq!(metas[0].poster.as_deref(), Some("https://images.example/poster/tt0000001.jpg"));
    assert_eq!(metas[0].release_info.as_deref(), Some("2026"));
    assert_eq!(metas[1].release_info.as_deref(), Some("2024"), "a year sent as a number");
    assert_eq!(metas[2].poster, None);
    assert_eq!(metas[3].kind, "series");
}

#[test]
fn an_empty_answer_is_an_empty_catalogue() {
    assert!(parse_catalog("{}").unwrap().is_empty());
    assert!(parse_catalog(r#"{"metas":[]}"#).unwrap().is_empty());
}

#[test]
fn bad_manifests_are_refused() {
    assert!(matches!(parse_manifest("<html>"), Err(Error::BadManifest(_))));
    assert!(matches!(parse_manifest(r#"{"name":"x"}"#), Err(Error::BadManifest(_))));
    assert!(matches!(
        parse_manifest(r#"{"id":" ","name":"x"}"#),
        Err(Error::IncompleteManifest("id"))
    ));
    let minimal = parse_manifest(r#"{"id":"a.b","name":"Tiny"}"#).unwrap();
    assert!(minimal.catalogs.is_empty());
}

#[test]
fn knows_which_catalogues_can_be_searched() {
    let manifest = parse_manifest(MANIFEST).unwrap();
    let searchable: Vec<_> = manifest
        .catalogs
        .iter()
        .filter(|catalog| catalog.searchable())
        .map(|catalog| (catalog.kind.as_str(), catalog.id.as_str()))
        .collect();
    assert_eq!(searchable, [("movie", "top")]);
    assert_eq!(manifest.catalogs[0].results_title(), "Films");
}

#[test]
fn knows_what_an_addon_can_be_asked_for() {
    let manifest = parse_manifest(MANIFEST).unwrap();
    // "meta" is in the short form: the manifest's types and id prefixes apply.
    assert!(manifest.provides("meta", "movie", "tt0000001"));
    assert!(manifest.provides("meta", "series", "tt0000004"));
    assert!(!manifest.provides("meta", "channel", "tt0000001"));
    assert!(!manifest.provides("meta", "movie", "kitsu:1"));
    // "stream" is in the full form and only covers films.
    assert!(manifest.provides("stream", "movie", "tt0000001"));
    assert!(!manifest.provides("stream", "series", "tt0000004:1:1"));
    assert!(!manifest.provides("subtitles", "movie", "tt0000001"));
}

#[test]
fn reads_details_with_old_and_new_spellings() {
    let meta = parse_meta(META).unwrap();
    assert_eq!(meta.name, "Long Runner");
    assert_eq!(meta.rating.as_deref(), Some("8.4"), "a rating sent as a number");
    assert_eq!(meta.genres, ["Drama", "Crime"], "the current spelling wins");
    assert_eq!(meta.cast, ["Ada Example", "Ben Sample"]);
    assert!(meta.director.is_empty(), "null is an empty list");
    assert_eq!(meta.videos.len(), 4, "entries with no id, or not objects, are left out");
}

#[test]
fn orders_seasons_and_episodes() {
    let meta = parse_meta(META).unwrap();
    assert_eq!(meta.seasons(), [1, 2, 0], "specials go last");
    let first: Vec<_> = meta
        .episodes(1)
        .iter()
        .map(|video| (video.episode, video.title.as_deref()))
        .collect();
    assert_eq!(first, [(Some(1), Some("Pilot")), (Some(2), Some("Second"))]);
    assert_eq!(meta.episodes(0)[0].id, "tt0000004:0:1", "season sent as text");
}

#[test]
fn a_missing_title_is_not_a_parse_error() {
    assert!(matches!(parse_meta("{}"), Err(Error::UnknownTitle)));
    assert!(matches!(parse_meta(r#"{"meta":{}}"#), Err(Error::UnknownTitle)));
    assert!(matches!(parse_meta(r#"{"meta":null}"#), Err(Error::UnknownTitle)));
    assert!(matches!(parse_meta("nope"), Err(Error::BadResponse(_))));
}

#[test]
fn reads_streams_and_tells_how_each_is_delivered() {
    let streams = parse_streams(STREAMS).unwrap();
    let kinds: Vec<_> = streams.iter().map(|stream| stream.kind().unwrap()).collect();
    assert_eq!(
        kinds,
        [
            StreamKind::Direct,
            StreamKind::Direct,
            StreamKind::External,
            StreamKind::YouTube,
            StreamKind::Torrent
        ],
        "entries that name no source are left out"
    );
    assert_eq!(streams[0].details(), Some("Direct file, 1.2 GB"));
    assert_eq!(streams[1].details(), Some("Older add-ons put the text here"));
    assert!(parse_streams("{}").unwrap().is_empty());
}

#[test]
fn repeats_of_a_title_are_dropped_and_the_clean_name_wins() {
    use haven_addons::{MetaPreview, title_key, without_repeats};
    let title = |id: &str, kind: &str, name: &str, year: Option<&str>| MetaPreview {
        id: id.into(),
        kind: kind.into(),
        name: name.into(),
        poster: None,
        release_info: year.map(Into::into),
        rating: None,
    };
    assert_eq!(
        title_key("movie", "Dune Part Two (2024) [1080p] [WEBRip] 88", None),
        title_key("movie", "Dune: Part Two", Some("2024"))
    );
    assert_ne!(title_key("movie", "Dune", Some("2021")), title_key("movie", "Dune", Some("1984")));
    assert_ne!(title_key("movie", "Dune", Some("2021")), title_key("series", "Dune", Some("2021")));

    let kept = without_repeats(vec![
        title("a", "movie", "Dune Part Two (2024) [1080p] [WEBRip] 88", None),
        title("b", "movie", "The Odyssey", Some("2026")),
        title("c", "movie", "Dune: Part Two", Some("2024")),
        title("d", "movie", "The Odyssey", Some("2026")),
        title("e", "movie", "Dune", Some("2021")),
        title("f", "series", "The Odyssey", Some("2026")),
    ]);
    let ids: Vec<&str> = kept.iter().map(|title| title.id.as_str()).collect();
    assert_eq!(ids, ["c", "b", "e", "f"]);
}

//! The audio and subtitle tracks of what is playing, read from mpv's
//! `track-list` property (which mpv hands out as JSON) and named for people.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Subtitle,
}

/// One audio or subtitle track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// mpv's number for the track within its kind; what `aid`/`sid` take.
    pub id: i64,
    pub kind: TrackKind,
    /// What to show in the menu, for example "English · Commentary · AC3 5.1".
    pub label: String,
    pub selected: bool,
}

#[derive(Deserialize)]
struct Raw {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    selected: bool,
    #[serde(default)]
    forced: bool,
    #[serde(default)]
    codec: Option<String>,
    #[serde(default, rename = "demux-channel-count")]
    channels: Option<i64>,
}

/// Reads mpv's `track-list`. Video tracks and entries that cannot be read
/// are left out; text that is not a track list gives no tracks.
pub fn parse_tracks(json: &str) -> Vec<Track> {
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| serde_json::from_value::<Raw>(entry).ok())
        .filter_map(|raw| {
            let kind = match raw.kind.as_str() {
                "audio" => TrackKind::Audio,
                "sub" => TrackKind::Subtitle,
                _ => return None,
            };
            Some(Track { id: raw.id, kind, label: label(&raw, kind), selected: raw.selected })
        })
        .collect()
}

fn label(raw: &Raw, kind: TrackKind) -> String {
    let text = |field: &Option<String>| {
        field.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned)
    };
    let language = text(&raw.lang).filter(|code| code != "und").map(|code| language_name(&code));
    let title = text(&raw.title);

    let mut parts: Vec<String> = Vec::new();
    parts.extend(language.clone());
    // Many files repeat the language as the title.
    if let Some(title) = title {
        if language.as_deref().is_none_or(|name| !name.eq_ignore_ascii_case(&title)) {
            parts.push(title);
        }
    }
    if parts.is_empty() {
        parts.push(format!("Track {}", raw.id));
    }
    if kind == TrackKind::Audio {
        let codec = text(&raw.codec).map(|codec| codec.to_uppercase());
        let layout = raw.channels.and_then(channel_layout);
        let sound: Vec<String> = codec.into_iter().chain(layout).collect();
        if !sound.is_empty() {
            parts.push(sound.join(" "));
        }
    }
    if raw.forced {
        parts.push("Forced".to_owned());
    }
    parts.join(" · ")
}

fn channel_layout(channels: i64) -> Option<String> {
    Some(match channels {
        1 => "Mono".to_owned(),
        2 => "Stereo".to_owned(),
        6 => "5.1".to_owned(),
        8 => "7.1".to_owned(),
        n if n > 0 => format!("{n} ch"),
        _ => return None,
    })
}

/// The English name of a language code as files carry them (two or three
/// letters, sometimes with a region: "en", "eng", "pt-BR"). Codes not in
/// the list are shown as they are, in capitals.
pub fn language_name(code: &str) -> String {
    let base = code.split(['-', '_']).next().unwrap_or(code).to_ascii_lowercase();
    let name = match base.as_str() {
        "en" | "eng" => "English",
        "af" | "afr" => "Afrikaans",
        "ar" | "ara" => "Arabic",
        "bg" | "bul" => "Bulgarian",
        "ca" | "cat" => "Catalan",
        "cs" | "ces" | "cze" => "Czech",
        "da" | "dan" => "Danish",
        "de" | "deu" | "ger" => "German",
        "el" | "ell" | "gre" => "Greek",
        "es" | "spa" => "Spanish",
        "et" | "est" => "Estonian",
        "fa" | "fas" | "per" => "Persian",
        "fi" | "fin" => "Finnish",
        "fr" | "fra" | "fre" => "French",
        "he" | "heb" => "Hebrew",
        "hi" | "hin" => "Hindi",
        "hr" | "hrv" => "Croatian",
        "hu" | "hun" => "Hungarian",
        "id" | "ind" => "Indonesian",
        "is" | "isl" | "ice" => "Icelandic",
        "it" | "ita" => "Italian",
        "ja" | "jpn" => "Japanese",
        "ko" | "kor" => "Korean",
        "lt" | "lit" => "Lithuanian",
        "lv" | "lav" => "Latvian",
        "ms" | "msa" | "may" => "Malay",
        "nb" | "nob" | "no" | "nor" => "Norwegian",
        "nl" | "nld" | "dut" => "Dutch",
        "pl" | "pol" => "Polish",
        "pt" | "por" => "Portuguese",
        "ro" | "ron" | "rum" => "Romanian",
        "ru" | "rus" => "Russian",
        "sk" | "slk" | "slo" => "Slovak",
        "sl" | "slv" => "Slovenian",
        "sr" | "srp" => "Serbian",
        "sv" | "swe" => "Swedish",
        "ta" | "tam" => "Tamil",
        "te" | "tel" => "Telugu",
        "th" | "tha" => "Thai",
        "tr" | "tur" => "Turkish",
        "uk" | "ukr" => "Ukrainian",
        "vi" | "vie" => "Vietnamese",
        "zh" | "zho" | "chi" => "Chinese",
        _ => return code.to_ascii_uppercase(),
    };
    name.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Made up in the shape mpv gives for `track-list`.
    const SAMPLE: &str = r#"[
        {"id": 1, "type": "video", "selected": true, "codec": "h264", "demux-w": 1920},
        {"id": 1, "type": "audio", "lang": "eng", "title": "English", "selected": true,
         "codec": "ac3", "demux-channel-count": 6},
        {"id": 2, "type": "audio", "lang": "eng", "title": "Commentary", "selected": false,
         "codec": "aac", "demux-channel-count": 2},
        {"id": 3, "type": "audio", "selected": false},
        {"id": 1, "type": "sub", "lang": "nl", "selected": false, "codec": "subrip"},
        {"id": 2, "type": "sub", "lang": "en", "title": "Signs", "forced": true, "selected": true},
        {"id": 3, "type": "sub", "lang": "und", "title": "  ", "selected": false},
        {"type": "sub"},
        "not a track"
    ]"#;

    #[test]
    fn names_audio_and_subtitle_tracks() {
        let tracks = parse_tracks(SAMPLE);
        let seen: Vec<(TrackKind, i64, &str, bool)> =
            tracks.iter().map(|t| (t.kind, t.id, t.label.as_str(), t.selected)).collect();
        assert_eq!(
            seen,
            [
                (TrackKind::Audio, 1, "English · AC3 5.1", true),
                (TrackKind::Audio, 2, "English · Commentary · AAC Stereo", false),
                (TrackKind::Audio, 3, "Track 3", false),
                (TrackKind::Subtitle, 1, "Dutch", false),
                (TrackKind::Subtitle, 2, "English · Signs · Forced", true),
                (TrackKind::Subtitle, 3, "Track 3", false),
            ]
        );
    }

    #[test]
    fn anything_that_is_not_a_track_list_gives_no_tracks() {
        assert!(parse_tracks("").is_empty());
        assert!(parse_tracks("{}").is_empty());
        assert!(parse_tracks("[]").is_empty());
    }

    #[test]
    fn unknown_language_codes_are_shown_as_they_are() {
        assert_eq!(language_name("pt-BR"), "Portuguese");
        assert_eq!(language_name("ENG"), "English");
        assert_eq!(language_name("tlh"), "TLH");
    }
}

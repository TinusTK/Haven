//! Add-ons are written by many hands: the same field can arrive as text,
//! a number, a list or nothing. These readers accept all of those.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

fn text(value: Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Text or a number, as text. Anything else, or blank text, is `None`.
pub fn text_or_number<'de, D: Deserializer<'de>>(de: D) -> Result<Option<String>, D::Error> {
    Ok(text(Value::deserialize(de)?))
}

/// A whole number sent as a number or as text.
pub fn number<'de, D: Deserializer<'de>>(de: D) -> Result<Option<u32>, D::Error> {
    Ok(match Value::deserialize(de)? {
        Value::Number(number) => number.as_u64().and_then(|n| u32::try_from(n).ok()),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    })
}

/// A list of text, a single text, or nothing.
pub fn texts<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<String>, D::Error> {
    Ok(match Value::deserialize(de)? {
        Value::Array(items) => items.into_iter().filter_map(text).collect(),
        other => text(other).into_iter().collect(),
    })
}

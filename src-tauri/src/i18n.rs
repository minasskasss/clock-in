//! The few strings Rust shows itself (tray menu, window titles), read from
//! the app's single wording file `src/i18n/el.json`, so all wording stays in
//! one place (CLAUDE.md).

use serde_json::Value;
use std::sync::OnceLock;

const EL_JSON: &str = include_str!("../../src/i18n/el.json");

fn strings() -> &'static Value {
    static STRINGS: OnceLock<Value> = OnceLock::new();
    STRINGS.get_or_init(|| serde_json::from_str(EL_JSON).unwrap_or(Value::Null))
}

/// The text at a dotted key such as `tray.quit`, or the key itself if it is
/// missing (a unit test checks every key Rust uses).
#[must_use]
pub fn text(key: &str) -> String {
    key.split('.')
        .try_fold(strings(), |value, part| value.get(part))
        .and_then(Value::as_str)
        .unwrap_or(key)
        .to_owned()
}

/// Every key Rust uses.
#[cfg(test)]
const KEYS: &[&str] = &[
    "app.name",
    "tray.open",
    "tray.quit",
    "tray.tooltip",
    "alarm.windowTitle",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rust_key_exists() {
        for key in KEYS {
            assert_ne!(text(key), *key, "missing in el.json: {key}");
        }
    }

    #[test]
    fn missing_keys_fall_back_to_the_key() {
        assert_eq!(text("no.such.key"), "no.such.key");
        assert_eq!(text("tray"), "tray", "not a string");
    }
}

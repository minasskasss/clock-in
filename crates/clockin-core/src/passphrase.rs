//! Admin passphrase format (SPEC §4.6): at least 5 words from the EFF large
//! wordlist, separated by spaces.
//!
//! Nothing here logs, stores or echoes a passphrase; errors carry only word
//! positions, never the words themselves.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

/// The EFF large wordlist as downloaded in Phase 0 (SHA-256 in DECISIONS).
const EFF_LARGE_WORDLIST: &str = include_str!("../../../assets/eff_large_wordlist.txt");

/// Number of words in the EFF large wordlist (6⁵ dice rolls).
pub const EFF_WORD_COUNT: usize = 7776;
/// Minimum number of words.
pub const MIN_WORDS: usize = 5;
/// Minimum length after normalisation (the server checks this too).
pub const MIN_CHARS: usize = 20;
/// Words in a generated passphrase.
pub const GENERATED_WORDS: usize = 5;

fn words() -> &'static [&'static str] {
    static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| {
        EFF_LARGE_WORDLIST
            .lines()
            .filter_map(|line| line.split('\t').nth(1))
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .collect()
    })
}

fn word_set() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| words().iter().copied().collect())
}

/// The word at `index` (0-based) of the EFF large wordlist.
#[must_use]
pub fn eff_word(index: usize) -> Option<&'static str> {
    words().get(index).copied()
}

/// Builds a passphrase from word indices chosen by the caller with the OS's
/// cryptographic random generator (each uniform in `0..EFF_WORD_COUNT`).
/// Returns `None` if any index is out of range.
#[must_use]
pub fn passphrase_from_indices(indices: &[usize]) -> Option<String> {
    let chosen: Option<Vec<&str>> = indices.iter().map(|&i| eff_word(i)).collect();
    Some(chosen?.join(" "))
}

/// Normalises typed input: Unicode NFC, lowercase, trimmed, any run of
/// whitespace collapsed to one space.
#[must_use]
pub fn normalize_passphrase(raw: &str) -> String {
    let nfc: String = raw.nfc().collect();
    nfc.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether two typed passphrases are the same once normalised (a "new"
/// passphrase equal to the current one is refused).
#[must_use]
pub fn same_passphrase(a: &str, b: &str) -> bool {
    normalize_passphrase(a) == normalize_passphrase(b)
}

/// Why a passphrase was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum PassphraseError {
    /// Fewer than 5 words.
    TooFewWords { count: usize },
    /// Fewer than 20 characters.
    TooShort,
    /// The words at these 0-based positions are not in the EFF list.
    UnknownWords { positions: Vec<usize> },
}

/// Normalises and checks a passphrase. Returns the normalised form, which is
/// what gets sent to the server.
///
/// # Errors
///
/// The first rule broken, in the order: word count, length, unknown words.
pub fn check_passphrase(raw: &str) -> Result<String, PassphraseError> {
    let normalized = normalize_passphrase(raw);
    let parts: Vec<&str> = normalized.split(' ').filter(|w| !w.is_empty()).collect();
    if parts.len() < MIN_WORDS {
        return Err(PassphraseError::TooFewWords { count: parts.len() });
    }
    if normalized.chars().count() < MIN_CHARS {
        return Err(PassphraseError::TooShort);
    }
    let set = word_set();
    let positions: Vec<usize> = parts
        .iter()
        .enumerate()
        .filter(|(_, w)| !set.contains(*w))
        .map(|(i, _)| i)
        .collect();
    if positions.is_empty() {
        Ok(normalized)
    } else {
        Err(PassphraseError::UnknownWords { positions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordlist_is_complete() {
        assert_eq!(words().len(), EFF_WORD_COUNT);
        assert_eq!(word_set().len(), EFF_WORD_COUNT, "no duplicates");
        assert_eq!(eff_word(0), Some("abacus"));
        assert_eq!(eff_word(EFF_WORD_COUNT - 1), Some("zoom"));
        assert_eq!(eff_word(EFF_WORD_COUNT), None);
        assert!(
            words()
                .iter()
                .all(|w| w.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'))
        );
    }

    #[test]
    fn normalisation() {
        assert_eq!(
            normalize_passphrase("  Abacus   ZOOM\tdrop-down \u{00A0} cloud\n"),
            "abacus zoom drop-down cloud"
        );
        assert_eq!(normalize_passphrase(""), "");
    }

    #[test]
    fn same_after_normalisation() {
        assert!(same_passphrase(
            "abacus zoom cloud tiger mango",
            "  Abacus ZOOM	cloud  tiger mango
"
        ));
        assert!(!same_passphrase(
            "abacus zoom cloud tiger mango",
            "abacus zoom cloud tiger melon"
        ));
        assert!(!same_passphrase(
            "abacus zoom cloud tiger mango",
            "zoom abacus cloud tiger mango"
        ));
    }

    #[test]
    fn valid_passphrases() {
        assert_eq!(
            check_passphrase("abacus zoom cloud tiger mango"),
            Ok("abacus zoom cloud tiger mango".to_string())
        );
        assert_eq!(
            check_passphrase("  ABACUS  zoom Cloud tiger mango  "),
            Ok("abacus zoom cloud tiger mango".to_string())
        );
        // Six words are fine too.
        assert!(check_passphrase("abacus zoom cloud tiger mango drop-down").is_ok());
    }

    #[test]
    fn too_few_words() {
        assert_eq!(
            check_passphrase("abacus zoom cloud tiger"),
            Err(PassphraseError::TooFewWords { count: 4 })
        );
        assert_eq!(
            check_passphrase("   "),
            Err(PassphraseError::TooFewWords { count: 0 })
        );
    }

    #[test]
    fn too_short() {
        // Five real three-letter EFF words: 19 characters.
        assert_eq!(
            check_passphrase("aim art cod elf emu"),
            Err(PassphraseError::TooShort)
        );
        // One more letter reaches 20.
        assert!(check_passphrase("aim art cod elf zoom").is_ok());
    }

    #[test]
    fn unknown_words_report_positions_only() {
        let err = check_passphrase("abacus zoomx cloud tiger mangoes").unwrap_err();
        assert_eq!(
            err,
            PassphraseError::UnknownWords {
                positions: vec![1, 4]
            }
        );
        let debug = format!("{err:?}");
        assert!(!debug.contains("zoomx") && !debug.contains("mangoes"));
    }

    #[test]
    fn greek_input_is_not_a_word() {
        assert_eq!(
            check_passphrase("αβγ abacus zoom cloud tiger"),
            Err(PassphraseError::UnknownWords { positions: vec![0] })
        );
    }

    #[test]
    fn generated_passphrases_are_valid() {
        let p = passphrase_from_indices(&[0, 1, 2, 3, EFF_WORD_COUNT - 1]).unwrap();
        assert_eq!(p, "abacus abdomen abdominal abide zoom");
        assert_eq!(check_passphrase(&p), Ok(p.clone()));
        assert_eq!(passphrase_from_indices(&[0, EFF_WORD_COUNT]), None);
    }
}

//! "Generate" in Settings (SPEC §4.6): a 5-word EFF passphrase picked with
//! the OS's cryptographic random generator. The result goes only to the
//! screen that asked for it; it is never logged or stored here.

use clockin_core::{EFF_WORD_COUNT, GENERATED_WORDS, passphrase_from_indices};

/// The largest multiple of the word count that fits in a `u16`; values at or
/// above it are drawn again so every word is equally likely.
const LIMIT: usize = (u16::MAX as usize + 1) / EFF_WORD_COUNT * EFF_WORD_COUNT;

/// # Errors
///
/// If the OS random generator fails.
pub fn generate() -> Result<String, getrandom::Error> {
    let mut failure = None;
    let words = from_random(|| {
        let mut bytes = [0u8; 2];
        match getrandom::fill(&mut bytes) {
            Ok(()) => Some(u16::from_le_bytes(bytes)),
            Err(e) => {
                failure = Some(e);
                None
            }
        }
    });
    match (words, failure) {
        (Some(words), _) => Ok(words),
        (None, Some(e)) => Err(e),
        (None, None) => Err(getrandom::Error::UNSUPPORTED),
    }
}

/// Builds the passphrase from a source of uniform `u16` values, by
/// rejection sampling. `None` if the source stops.
fn from_random(mut next: impl FnMut() -> Option<u16>) -> Option<String> {
    let mut indices = Vec::with_capacity(GENERATED_WORDS);
    while indices.len() < GENERATED_WORDS {
        let value = usize::from(next()?);
        if value < LIMIT {
            indices.push(value % EFF_WORD_COUNT);
        }
    }
    passphrase_from_indices(&indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clockin_core::check_passphrase;

    #[test]
    fn generated_passphrases_are_five_valid_words() {
        for _ in 0..20 {
            let p = generate().unwrap();
            assert_eq!(p.split(' ').count(), GENERATED_WORDS);
            assert_eq!(check_passphrase(&p).as_deref(), Ok(p.as_str()));
        }
        assert_ne!(generate().unwrap(), generate().unwrap());
    }

    #[test]
    fn rejection_sampling_skips_the_biased_tail() {
        assert_eq!(LIMIT, 62_208);
        // 65535 is rejected; 0 → first word, 7776 → first word again,
        // 7775 → last word.
        let mut values = vec![65_535, 0, 7_776, 7_775, 62_207, 1].into_iter();
        let p = from_random(|| values.next()).unwrap();
        let words: Vec<_> = p.split(' ').collect();
        assert_eq!(words[0], "abacus");
        assert_eq!(words[1], "abacus");
        assert_eq!(words[2], "zoom");
        assert_eq!(words[4], "abdomen");
        assert_eq!(from_random(|| None), None);
    }
}

//! Credentials that must never reach logs, panics or error messages
//! (CLAUDE.md, "Never log credentials"). Their `Debug` output is redacted and
//! they have no `Display`; [`expose`](DeviceSecret::expose) is the only way
//! to the value.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! secret_string {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            #[must_use]
            pub fn new(value: String) -> Self {
                Self(value)
            }

            /// The raw value, for sending to the server or storing in the
            /// platform's secret store. Never log it.
            #[must_use]
            pub fn expose(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

secret_string!(
    /// The per-device secret returned once by `pair_device` / `admin_initialize`.
    DeviceSecret
);
secret_string!(
    /// An admin session token returned by `admin_login`.
    SessionToken
);
secret_string!(
    /// The 4-digit quit code. Not a security control (SPEC §8.1), but it is
    /// still kept out of logs.
    QuitCode
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_is_redacted() {
        let s = DeviceSecret::new("super-secret-value".into());
        let t = SessionToken::new("token-value".into());
        let q = QuitCode::new("4321".into());
        let all = format!("{s:?} {t:?} {q:?} {:?}", Some(&s));
        assert!(!all.contains("super-secret-value"));
        assert!(!all.contains("token-value"));
        assert!(!all.contains("4321"));
        assert!(all.contains("DeviceSecret(<redacted>)"));
        assert_eq!(s.expose(), "super-secret-value");
    }

    #[test]
    fn serialises_as_a_plain_string() {
        let s = DeviceSecret::new("abc".into());
        assert_eq!(serde_json::to_string(&s).unwrap(), r#""abc""#);
        let back: DeviceSecret = serde_json::from_str(r#""abc""#).unwrap();
        assert_eq!(back, s);
    }
}

//! Which local profile this instance uses (ARCHITECTURE §9, debug only).
//!
//! A debug build started with `--profile <name>` keeps its own data folder,
//! webview data and credentials, so two instances can pair as two devices
//! on one PC for sync testing. Release builds ignore the flag.

// Debug profiles are a desktop feature; Android uses only the default one.
#![cfg_attr(mobile, allow(dead_code))]

use std::path::{Path, PathBuf};

/// The credential namespace of the default profile: the app identifier.
const SERVICE: &str = "io.github.minasskasss.clockin";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    name: Option<String>,
}

impl Profile {
    /// The profile from the command line (debug builds only).
    #[must_use]
    pub fn from_args() -> Self {
        if cfg!(debug_assertions) {
            Self::parse(std::env::args().skip(1))
        } else {
            Self::default()
        }
    }

    /// `--profile <name>` or `--profile=<name>`; names are 1–20 characters of
    /// `a-z`, `0-9` and `-`. Anything else means the default profile.
    fn parse(args: impl Iterator<Item = String>) -> Self {
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            let value = if arg == "--profile" {
                args.next()
            } else {
                arg.strip_prefix("--profile=").map(str::to_owned)
            };
            if let Some(value) = value {
                let name = value.trim().to_lowercase();
                let valid = (1..=20).contains(&name.len())
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
                return Self {
                    name: (valid && name != "default").then_some(name),
                };
            }
        }
        Self::default()
    }

    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The data folder inside the app's local data folder.
    #[must_use]
    pub fn data_dir(&self, base: &Path) -> PathBuf {
        match &self.name {
            None => base.to_path_buf(),
            Some(name) => base.join(format!("profile-{name}")),
        }
    }

    /// The credential-store service name.
    #[must_use]
    pub fn credential_service(&self) -> String {
        match &self.name {
            None => SERVICE.to_owned(),
            Some(name) => format!("{SERVICE}.profile-{name}"),
        }
    }

    /// The suggested device name: the computer's name (plus the profile).
    #[must_use]
    pub fn default_device_name(&self) -> String {
        let host = ["COMPUTERNAME", "HOSTNAME"]
            .iter()
            .find_map(|var| std::env::var(var).ok().filter(|v| !v.trim().is_empty()))
            .unwrap_or_else(|| "Clock In".to_owned());
        let name = match &self.name {
            None => host.trim().to_owned(),
            Some(profile) => format!("{} ({profile})", host.trim()),
        };
        name.chars().take(60).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Profile {
        Profile::parse(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn parses_the_profile_flag() {
        assert_eq!(parse(&[]).name(), None);
        assert_eq!(parse(&["--profile", "b"]).name(), Some("b"));
        assert_eq!(parse(&["--profile=Test-2"]).name(), Some("test-2"));
        assert_eq!(parse(&["--other", "--profile", "b"]).name(), Some("b"));
        for bad in [
            &["--profile"][..],
            &["--profile", ""],
            &["--profile", "../x"],
            &["--profile", "default"],
        ] {
            assert_eq!(parse(bad).name(), None, "{bad:?}");
        }
        assert_eq!(parse(&["--profile", &"x".repeat(21)]).name(), None);
    }

    #[test]
    fn profiles_get_their_own_folder_and_credentials() {
        let base = Path::new("data");
        let default = Profile::default();
        let b = parse(&["--profile", "b"]);
        assert_eq!(default.data_dir(base), base);
        assert_eq!(b.data_dir(base), base.join("profile-b"));
        assert_eq!(default.credential_service(), SERVICE);
        assert_eq!(b.credential_service(), format!("{SERVICE}.profile-b"));
        assert!(b.default_device_name().ends_with(" (b)"));
        assert!(b.default_device_name().chars().count() <= 60);
    }
}

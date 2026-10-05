//! The OS secret store for the device secret and the local database key
//! (ARCHITECTURE §5.7).
//!
//! - Windows: Windows Credential Manager (DPAPI, user-scoped), as "generic
//!   credentials" with Local persistence (this computer only).
//! - Android: Phase 5 adds a Keystore-backed store through the alarm plugin.
//! - Other desktops (Linux CI, development only): files in the app's data
//!   folder. Not a supported platform and not protected.
//!
//! Errors never contain the secret values.

#[derive(Debug, thiserror::Error)]
#[error("the OS secret store failed: {0}")]
pub struct SecretError(String);

/// Named secrets of one profile.
pub trait SecretStore: Send + Sync {
    /// # Errors
    ///
    /// If the store can't be read.
    fn get(&self, name: &str) -> Result<Option<String>, SecretError>;

    /// # Errors
    ///
    /// If the store can't be written.
    fn set(&self, name: &str, value: &str) -> Result<(), SecretError>;

    /// Deleting a missing secret is fine.
    ///
    /// # Errors
    ///
    /// If the store can't be written.
    fn delete(&self, name: &str) -> Result<(), SecretError>;
}

#[cfg(windows)]
pub use windows::CredentialManager as PlatformSecrets;

#[cfg(windows)]
mod windows {
    use super::{SecretError, SecretStore};
    use keyring_core::api::CredentialStoreApi;
    use keyring_core::{Entry, Error};
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Windows Credential Manager, one generic credential per secret, named
    /// `<name>.<service>`.
    pub struct CredentialManager {
        store: Arc<windows_native_keyring_store::Store>,
        service: String,
    }

    impl CredentialManager {
        /// # Errors
        ///
        /// If the credential store can't be opened.
        pub fn new(service: String, _data_dir: &std::path::Path) -> Result<Self, SecretError> {
            let store = windows_native_keyring_store::Store::new()
                .map_err(|e| SecretError(e.to_string()))?;
            Ok(Self { store, service })
        }

        fn entry(&self, name: &str) -> Result<Entry, SecretError> {
            // Local: kept on this computer, not roamed with the user profile.
            let modifiers = HashMap::from([("persistence", "Local")]);
            self.store
                .build(&self.service, name, Some(&modifiers))
                .map_err(|e| SecretError(e.to_string()))
        }
    }

    impl SecretStore for CredentialManager {
        fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
            match self.entry(name)?.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(Error::NoEntry) => Ok(None),
                Err(e) => Err(SecretError(e.to_string())),
            }
        }

        fn set(&self, name: &str, value: &str) -> Result<(), SecretError> {
            self.entry(name)?
                .set_password(value)
                .map_err(|e| SecretError(e.to_string()))
        }

        fn delete(&self, name: &str) -> Result<(), SecretError> {
            match self.entry(name)?.delete_credential() {
                Ok(()) | Err(Error::NoEntry) => Ok(()),
                Err(e) => Err(SecretError(e.to_string())),
            }
        }
    }
}

#[cfg(all(not(windows), not(target_os = "android")))]
pub use files::FileSecrets as PlatformSecrets;

#[cfg(all(not(windows), not(target_os = "android")))]
mod files {
    use super::{SecretError, SecretStore};
    use std::path::{Path, PathBuf};

    /// Development-only stand-in for desktops other than Windows.
    pub struct FileSecrets {
        dir: PathBuf,
    }

    impl FileSecrets {
        /// # Errors
        ///
        /// If the folder can't be created.
        pub fn new(_service: String, data_dir: &Path) -> Result<Self, SecretError> {
            let dir = data_dir.join("secrets");
            std::fs::create_dir_all(&dir).map_err(|e| SecretError(e.kind().to_string()))?;
            Ok(Self { dir })
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.join(name)
        }
    }

    impl SecretStore for FileSecrets {
        fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
            match std::fs::read_to_string(self.path(name)) {
                Ok(value) => Ok(Some(value)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(SecretError(e.kind().to_string())),
            }
        }

        fn set(&self, name: &str, value: &str) -> Result<(), SecretError> {
            std::fs::write(self.path(name), value).map_err(|e| SecretError(e.kind().to_string()))
        }

        fn delete(&self, name: &str) -> Result<(), SecretError> {
            match std::fs::remove_file(self.path(name)) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(SecretError(e.kind().to_string())),
            }
        }
    }
}

/// An in-memory store for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemorySecrets(std::sync::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        Ok(self.0.lock().unwrap().get(name).cloned())
    }

    fn set(&self, name: &str, value: &str) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(name.into(), value.into());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Writes, reads and deletes a throwaway credential in the real
    /// Credential Manager (namespace `…clockin.test-<uuid>`).
    #[test]
    fn credential_manager_round_trip() {
        let service = format!(
            "io.github.minasskasss.clockin.test-{}",
            uuid::Uuid::new_v4()
        );
        let store = PlatformSecrets::new(service, std::path::Path::new(".")).unwrap();
        assert_eq!(store.get("x").unwrap(), None);
        store.set("x", "value-1").unwrap();
        assert_eq!(store.get("x").unwrap().as_deref(), Some("value-1"));
        store.set("x", "value-2").unwrap();
        assert_eq!(store.get("x").unwrap().as_deref(), Some("value-2"));
        store.delete("x").unwrap();
        assert_eq!(store.get("x").unwrap(), None);
        store.delete("x").unwrap();
    }
}

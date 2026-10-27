//! Credentials kept on this computer, outside the company file and its backups: the operating
//! system's credential store (the macOS Keychain or Windows Credential Manager), or a JSON file
//! readable by its owner only elsewhere (and in tests).
//!
//! Only the installed app uses the system store. Debug builds and builds under any other bundle
//! identifier (the test builds parallel work launches) keep their secrets in files in their own
//! config folder, so a test launch never prompts for the Keychain password and never reads or
//! overwrites the installed app's secrets. `store_kind` makes that choice.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The bundle identifier of the installed app (`identifier` in `tauri.conf.json`).
pub const PRODUCTION_IDENTIFIER: &str = "com.localbooks.desktop";

/// Where a build keeps its secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreKind {
    /// The macOS Keychain or Windows Credential Manager (where available).
    System,
    /// Owner-only JSON files in the build's own config folder.
    File,
}

/// The production app (a release build under the production identifier) uses the system store;
/// every other build uses files.
pub fn store_kind(identifier: &str, debug_build: bool) -> StoreKind {
    if !debug_build && identifier == PRODUCTION_IDENTIFIER {
        StoreKind::System
    } else {
        StoreKind::File
    }
}

static THIS_BUILD: OnceLock<StoreKind> = OnceLock::new();

/// Records which store this build uses, from the running app's bundle identifier. Called first at
/// startup; until then (tests, the command-line tools) `for_this_computer` uses files.
pub fn install_for_this_build(identifier: &str) -> StoreKind {
    *THIS_BUILD.get_or_init(|| store_kind(identifier, cfg!(debug_assertions)))
}

#[derive(Debug, Clone)]
enum Backend {
    /// The macOS Keychain or Windows Credential Manager.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    System,
    /// A JSON file readable by its owner only (Linux, development builds and tests).
    File(PathBuf),
}

/// Secrets of one kind, by key. `what` names them in errors ("the bank connection").
#[derive(Debug, Clone)]
pub struct SecretStore {
    backend: Backend,
    /// The credential store's service name.
    service: &'static str,
    what: &'static str,
}

impl SecretStore {
    /// The system store under `service` in the production app on macOS and Windows; otherwise
    /// `file_name` in the config folder.
    pub fn for_this_computer(config_dir: &Path, service: &'static str, file_name: &str, what: &'static str) -> Self {
        Self::for_kind(THIS_BUILD.get().copied().unwrap_or(StoreKind::File), config_dir, service, file_name, what)
    }

    fn for_kind(kind: StoreKind, config_dir: &Path, service: &'static str, file_name: &str, what: &'static str) -> Self {
        match kind {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            StoreKind::System => SecretStore { backend: Backend::System, service, what },
            _ => SecretStore::in_file(config_dir.join(file_name), service, what),
        }
    }

    pub fn in_file(path: PathBuf, service: &'static str, what: &'static str) -> Self {
        SecretStore { backend: Backend::File(path), service, what }
    }

    fn read_file(path: &Path) -> Result<BTreeMap<String, String>, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("SECRETS_FILE: {} is damaged: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(format!("SECRETS_FILE: cannot read {}: {e}", path.display())),
        }
    }

    fn write_file(path: &Path, map: &BTreeMap<String, String>) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("SECRETS_FILE: cannot create {}: {e}", dir.display()))?;
        }
        let json = serde_json::to_vec_pretty(map).map_err(|e| format!("SECRETS_FILE: {e}"))?;
        crate::instance::write_private(path, &json).map_err(|e| format!("SECRETS_FILE: cannot write {}: {e}", path.display()))
    }

    fn get_blocking(&self, key: &str) -> Result<Option<String>, String> {
        match &self.backend {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Backend::System => match keyring::Entry::new(self.service, key).and_then(|e| e.get_password()) {
                Ok(secret) => Ok(Some(secret)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(format!("SECRETS_KEYCHAIN: cannot read {}: {e}", self.what)),
            },
            Backend::File(path) => Ok(Self::read_file(path)?.remove(key)),
        }
    }

    fn put_blocking(&self, key: &str, secret: &str) -> Result<(), String> {
        match &self.backend {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Backend::System => keyring::Entry::new(self.service, key)
                .and_then(|e| e.set_password(secret))
                .map_err(|e| format!("SECRETS_KEYCHAIN: cannot save {}: {e}", self.what)),
            Backend::File(path) => {
                let mut map = Self::read_file(path)?;
                map.insert(key.to_string(), secret.to_string());
                Self::write_file(path, &map)
            }
        }
    }

    fn delete_blocking(&self, key: &str) -> Result<(), String> {
        match &self.backend {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Backend::System => match keyring::Entry::new(self.service, key).and_then(|e| e.delete_credential()) {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(format!("SECRETS_KEYCHAIN: cannot remove {}: {e}", self.what)),
            },
            Backend::File(path) => {
                let mut map = Self::read_file(path)?;
                if map.remove(key).is_some() {
                    Self::write_file(path, &map)?;
                }
                Ok(())
            }
        }
    }

    // The system store may wait on the user (a Keychain permission prompt), so it runs off the
    // async workers.
    pub async fn get(&self, key: &str) -> Result<Option<String>, String> {
        let (store, key) = (self.clone(), key.to_string());
        tokio::task::spawn_blocking(move || store.get_blocking(&key)).await.map_err(|e| format!("SECRETS_KEYCHAIN: {e}"))?
    }

    pub async fn put(&self, key: &str, secret: &str) -> Result<(), String> {
        let (store, key, secret) = (self.clone(), key.to_string(), secret.to_string());
        tokio::task::spawn_blocking(move || store.put_blocking(&key, &secret)).await.map_err(|e| format!("SECRETS_KEYCHAIN: {e}"))?
    }

    pub async fn delete(&self, key: &str) -> Result<(), String> {
        let (store, key) = (self.clone(), key.to_string());
        tokio::task::spawn_blocking(move || store.delete_blocking(&key)).await.map_err(|e| format!("SECRETS_KEYCHAIN: {e}"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_production_app_uses_the_system_store() {
        assert_eq!(store_kind(PRODUCTION_IDENTIFIER, false), StoreKind::System);
        assert_eq!(store_kind(PRODUCTION_IDENTIFIER, true), StoreKind::File);
        assert_eq!(store_kind("com.localbooks.mileage", false), StoreKind::File);
        assert_eq!(store_kind("com.localbooks.mileage", true), StoreKind::File);
        assert_eq!(store_kind("", false), StoreKind::File);
    }

    #[test]
    fn the_production_identifier_matches_tauri_conf() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], PRODUCTION_IDENTIFIER);
    }

    #[test]
    fn a_file_store_lives_in_the_builds_config_folder() {
        let dir = std::env::temp_dir().join(format!("localbooks-secrets-{}", std::process::id()));
        let store = SecretStore::for_kind(StoreKind::File, &dir, "LocalBooks SMTP", "smtp-secrets.json", "the email password");
        assert!(matches!(&store.backend, Backend::File(path) if *path == dir.join("smtp-secrets.json")));
        // Before the build is installed (tests, command-line tools), nothing reaches the system store.
        let store = SecretStore::for_this_computer(&dir, "LocalBooks SMTP", "smtp-secrets.json", "the email password");
        assert!(matches!(&store.backend, Backend::File(_)));
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn the_production_kind_uses_the_system_store() {
        let store = SecretStore::for_kind(StoreKind::System, Path::new("unused"), "LocalBooks SMTP", "smtp-secrets.json", "the email password");
        assert!(matches!(store.backend, Backend::System));
    }
}

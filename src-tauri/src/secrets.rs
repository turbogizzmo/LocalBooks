//! Credentials kept on this computer, outside the company file and its backups: the operating
//! system's credential store (the macOS Keychain or Windows Credential Manager), or a JSON file
//! readable by its owner only elsewhere (and in tests).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
enum Backend {
    /// The macOS Keychain or Windows Credential Manager.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    System,
    /// A JSON file readable by its owner only (Linux, and tests).
    #[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
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
    /// The system store under `service`; on Linux, `file_name` in the config folder.
    pub fn for_this_computer(config_dir: &Path, service: &'static str, file_name: &str, what: &'static str) -> Self {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let _ = (config_dir, file_name);
            SecretStore { backend: Backend::System, service, what }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            SecretStore::in_file(config_dir.join(file_name), service, what)
        }
    }

    #[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
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

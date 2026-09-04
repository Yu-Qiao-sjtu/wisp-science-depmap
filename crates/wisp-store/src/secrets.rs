//! OS keyring-backed secret storage for API keys.
//!
//! Windows debug builds use the same OS credential manager as release builds,
//! which lets `tauri dev` reproduce a configured installation without copying
//! real provider keys into source files or plaintext developer configuration.
//! Other debug targets keep the isolated test/development file backend: macOS
//! binds keychain entries to the calling app's changing development signature,
//! while CI must not require a real keyring daemon.

/// A named secret (e.g. an API key) stored in the OS credential manager.
pub struct Secret;

impl Secret {
    pub fn set(name: &str, value: &str) -> anyhow::Result<()> {
        backend::set(name, value)
    }

    pub fn get(name: &str) -> anyhow::Result<String> {
        backend::get(name)
    }

    pub fn delete(name: &str) -> anyhow::Result<()> {
        backend::delete(name)
    }
}

#[cfg(any(not(debug_assertions), target_os = "windows"))]
mod backend {
    use keyring::Entry;

    const SERVICE: &str = "wisp";
    #[cfg(test)]
    pub const KIND: &str = "os-keyring";

    pub fn set(name: &str, value: &str) -> anyhow::Result<()> {
        Entry::new(SERVICE, name)?.set_password(value)?;
        Ok(())
    }

    pub fn get(name: &str) -> anyhow::Result<String> {
        Ok(Entry::new(SERVICE, name)?.get_password()?)
    }

    pub fn delete(name: &str) -> anyhow::Result<()> {
        Entry::new(SERVICE, name)?.delete_credential()?;
        Ok(())
    }
}

#[cfg(all(debug_assertions, not(target_os = "windows")))]
mod backend {
    // Dev-only plaintext file. Serialize load+store so parallel `cargo test`
    // workers cannot clobber each other's whole-file rewrites.
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    #[cfg(test)]
    pub const KIND: &str = "isolated-debug-file";

    fn file() -> PathBuf {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(".wisp-science-dev-secrets.json")
    }

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn load() -> BTreeMap<String, String> {
        std::fs::read(file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn store(map: &BTreeMap<String, String>) -> anyhow::Result<()> {
        std::fs::write(file(), serde_json::to_vec_pretty(map)?)?;
        Ok(())
    }

    pub fn set(name: &str, value: &str) -> anyhow::Result<()> {
        let _guard = lock();
        let mut map = load();
        map.insert(name.to_string(), value.to_string());
        store(&map)
    }

    pub fn get(name: &str) -> anyhow::Result<String> {
        let _guard = lock();
        load()
            .remove(name)
            .ok_or_else(|| anyhow::anyhow!("no secret named {name}"))
    }

    pub fn delete(name: &str) -> anyhow::Result<()> {
        let _guard = lock();
        let mut map = load();
        map.remove(name);
        store(&map)
    }
}

#[cfg(test)]
mod backend_policy_tests {
    #[test]
    fn platform_selects_the_intended_secret_backend() {
        #[cfg(target_os = "windows")]
        assert_eq!(super::backend::KIND, "os-keyring");

        #[cfg(all(debug_assertions, not(target_os = "windows")))]
        assert_eq!(super::backend::KIND, "isolated-debug-file");

        #[cfg(all(not(debug_assertions), not(target_os = "windows")))]
        assert_eq!(super::backend::KIND, "os-keyring");
    }
}

#[cfg(all(test, debug_assertions, not(target_os = "windows")))]
mod tests {
    use super::Secret;

    // Exercises only the debug file backend (cargo test builds with
    // debug_assertions), so no OS keyring daemon is ever required. The entry
    // name is UUID-scoped so parallel test runs sharing $HOME never collide.
    #[test]
    fn set_get_delete_roundtrip() {
        let name = format!("test:roundtrip:{}", uuid::Uuid::new_v4());
        Secret::set(&name, "abc123").unwrap();
        assert_eq!(Secret::get(&name).unwrap(), "abc123");
        Secret::delete(&name).unwrap();
        assert!(Secret::get(&name).is_err());
    }
}

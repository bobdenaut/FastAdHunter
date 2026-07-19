//! The single API key (SECURITY.md §API access): generated on first boot,
//! printed once to the log, stored in `/config`, rotatable, old key invalid
//! immediately.

use std::fs;
use std::path::{Path, PathBuf};

use arc_swap::ArcSwap;
use rand::RngCore;
use std::sync::Arc;

/// Key length in bytes before hex encoding. 32 bytes = 256 bits of entropy,
/// well past brute-force reach for a bearer token that never expires.
const KEY_BYTES: usize = 32;

const KEY_FILE: &str = "apikey";

pub struct ApiKeyStore {
    path: PathBuf,
    current: ArcSwap<String>,
}

impl ApiKeyStore {
    /// Loads the stored key, or generates and persists one on first boot.
    /// The generated key is returned so the caller can print it once
    /// (SECURITY.md: "printed once to the container log").
    pub fn load_or_create(config_dir: &Path) -> std::io::Result<(Self, Option<String>)> {
        let path = config_dir.join(KEY_FILE);
        if let Ok(existing) = fs::read_to_string(&path) {
            let existing = existing.trim().to_string();
            if !existing.is_empty() {
                return Ok((
                    Self {
                        path,
                        current: ArcSwap::from_pointee(existing),
                    },
                    None,
                ));
            }
        }

        let key = generate();
        write_key(&path, &key)?;
        Ok((
            Self {
                path,
                current: ArcSwap::from_pointee(key.clone()),
            },
            Some(key),
        ))
    }

    /// An in-memory store for tests — no `/config` involvement.
    #[cfg(test)]
    pub fn in_memory(key: &str) -> Self {
        Self {
            path: PathBuf::from("apikey"),
            current: ArcSwap::from_pointee(key.to_string()),
        }
    }

    pub fn current(&self) -> Arc<String> {
        self.current.load_full()
    }

    /// Generates a replacement, persists it, and swaps it in — the old key
    /// stops working the moment the swap lands
    /// (`POST /api/v1/config/apikey/rotate`).
    pub fn rotate(&self) -> std::io::Result<String> {
        let key = generate();
        write_key(&self.path, &key)?;
        self.current.store(Arc::new(key.clone()));
        Ok(key)
    }

    /// Constant-time comparison: a byte-by-byte early return would leak the
    /// shared prefix length, letting a caller recover the key one byte at a
    /// time. Not crypto — just a comparison that does not branch on content
    /// (SECURITY.md's "no hand-rolled crypto" bars primitives, not this).
    pub fn matches(&self, candidate: &str) -> bool {
        let current = self.current();
        let (expected, got) = (current.as_bytes(), candidate.as_bytes());
        if expected.len() != got.len() {
            return false;
        }
        let mut diff = 0u8;
        for (a, b) in expected.iter().zip(got.iter()) {
            diff |= a ^ b;
        }
        diff == 0
    }
}

fn generate() -> String {
    let mut bytes = [0u8; KEY_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Writes the key with owner-only permissions where the platform supports it
/// (SECURITY.md §Data at rest: "file permissions restricted to the container
/// user").
fn write_key(path: &Path, key: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, key)?;
    restrict_permissions(path)
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> std::io::Result<()> {
    // Windows ACLs are not the deployment target (the container is Linux);
    // the file lives in the user's own directory there.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_boot_generates_persists_and_reports_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let (store, fresh) = ApiKeyStore::load_or_create(dir.path()).unwrap();
        let key = fresh.expect("first boot must report the generated key once");
        assert_eq!(key.len(), KEY_BYTES * 2);
        assert_eq!(*store.current(), key);

        // A restart loads the same key and reports nothing new.
        let (reloaded, fresh) = ApiKeyStore::load_or_create(dir.path()).unwrap();
        assert!(fresh.is_none(), "an existing key is not re-announced");
        assert_eq!(*reloaded.current(), key);
    }

    #[test]
    fn rotation_replaces_the_key_and_invalidates_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = ApiKeyStore::load_or_create(dir.path()).unwrap();
        let old = (*store.current()).clone();

        let new = store.rotate().unwrap();
        assert_ne!(new, old);
        assert!(store.matches(&new));
        assert!(
            !store.matches(&old),
            "the old key must stop working at once"
        );

        // Persisted, so a restart honors the rotation.
        let (reloaded, _) = ApiKeyStore::load_or_create(dir.path()).unwrap();
        assert_eq!(*reloaded.current(), new);
    }

    #[test]
    fn matching_rejects_wrong_and_differently_sized_candidates() {
        let store = ApiKeyStore::in_memory("abcdef");
        assert!(store.matches("abcdef"));
        assert!(!store.matches("abcdeg"));
        assert!(!store.matches("abcde"));
        assert!(!store.matches("abcdefg"));
        assert!(!store.matches(""));
    }

    #[test]
    fn generated_keys_do_not_repeat() {
        let a = generate();
        let b = generate();
        assert_ne!(a, b);
    }
}

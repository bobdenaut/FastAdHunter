//! The live configuration behind `GET`/`POST /api/v1/config`.
//!
//! CONFIGURATION.md fixes the contract: a `POST` is a *partial* update
//! deep-merged onto the effective config, validated, written back to
//! `/config/fastadhunter.toml` ("no hidden state; the file always reflects
//! the running intent"), then classified — runtime keys apply live via atomic
//! swap, boot keys persist and answer `restart_required: true`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use fah_config::Config;
use serde_json::Value;

/// Keys whose class is **boot** (CONFIGURATION.md's `# boot` comments).
/// Everything not listed is runtime-mutable. Dotted paths, matched as
/// prefixes so `dns.cache` covers every field in the section.
///
/// The bar for leaving a key out of this list is not "it would be nice to
/// change it live" — it is that something actually re-reads it after a patch.
/// Only three keys clear it today, each with a real consumer: `history.enabled`
/// and `history.retention_days` (pushed through `apply_history_config` into
/// the writers' shared atomic), `rules.refresh_hours_default` (read per
/// request by the lists handlers), and since p2-06 `schedule.timezone` and
/// `policies` (recompiled and republished into the live client → policy
/// snapshot). Everything else is consumed once during
/// boot — `DnsCache::new`, `UpstreamPool::from_config`, `Pipeline::new`,
/// `Stats::new`, the tracing filter — so answering `restart_required: false`
/// for it would report an apply that never happened.
///
/// Sections are listed whole rather than field-by-field on purpose: a field
/// added to `[dns.cache]` tomorrow is boot until someone wires it live, which
/// is the safe default for this contract.
const BOOT_KEYS: [&str; 16] = [
    "engine.mode",
    "runtime",
    "dns.listen",
    "dns.blocking",
    "dns.cache",
    "dns.upstreams",
    // Whole section, per the note above. `max_connections` looks runtime-shaped
    // and is not: the semaphore is sized once at `fah_http::Server::bind`.
    // p2-02 may promote it — by giving it a live consumer first, never by
    // moving it out of this list and hoping.
    "http",
    "https",
    // Whole section: the allow-list is parsed into a `DestinationPolicy` once,
    // when the binary builds the proxy. Applying it live would mean swapping a
    // security policy under in-flight connections — if that is ever wanted it
    // needs an atomic swap and a test, not a reclassification here.
    "egress",
    "stats",
    "history.sample_interval_seconds",
    "api.address",
    "api.port",
    "api.tls",
    "log.level",
    "log.format",
];

#[derive(Debug, thiserror::Error)]
pub enum ConfigStoreError {
    #[error("{0}")]
    Invalid(String),
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        /// Boxed: `ConfigError` carries TOML spans and is large enough that
        /// inlining it here would bloat every `Result` in this module
        /// (clippy's `result_large_err`).
        #[source]
        source: Box<fah_config::ConfigError>,
    },
}

/// The outcome of a merge, mirroring `POST /api/v1/config`'s response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateOutcome {
    /// At least one runtime-mutable key changed and is now live.
    pub applied: bool,
    /// At least one boot-only key changed: persisted, awaiting a restart.
    pub restart_required: bool,
}

pub struct ConfigStore {
    path: PathBuf,
    current: ArcSwap<Config>,
}

impl ConfigStore {
    pub fn new(config: Config, path: PathBuf) -> Self {
        Self {
            path,
            current: ArcSwap::from_pointee(config),
        }
    }

    pub fn current(&self) -> Arc<Config> {
        self.current.load_full()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Applies a partial update. Validation happens before anything is
    /// persisted or swapped, so a rejected patch leaves both the file and the
    /// running config untouched.
    pub fn apply_patch(&self, patch: &Value) -> Result<UpdateOutcome, ConfigStoreError> {
        if !patch.is_object() {
            return Err(ConfigStoreError::Invalid(
                "config patch must be a JSON object".to_string(),
            ));
        }

        let current = self.current();
        let mut merged = serde_json::to_value(current.as_ref()).map_err(|err| {
            ConfigStoreError::Invalid(format!("serializing current config: {err}"))
        })?;
        merge(&mut merged, patch);

        // `deny_unknown_fields` on the schema turns a typo'd key into an
        // error here rather than a silently ignored setting.
        let candidate: Config = serde_json::from_value(merged)
            .map_err(|err| ConfigStoreError::Invalid(err.to_string()))?;
        candidate
            .validate()
            .map_err(|err| ConfigStoreError::Invalid(err.to_string()))?;

        let changed = changed_paths(current.as_ref(), &candidate)?;
        let restart_required = changed.iter().any(|path| is_boot_key(path));
        let applied = changed.iter().any(|path| !is_boot_key(path));

        candidate
            .save(&self.path)
            .map_err(|source| ConfigStoreError::Write {
                path: self.path.clone(),
                source: Box::new(source),
            })?;
        self.current.store(Arc::new(candidate));

        Ok(UpdateOutcome {
            applied,
            restart_required,
        })
    }
}

/// Recursive object merge: nested objects merge key-by-key, everything else
/// (scalars, and arrays such as `[[rules.lists]]`) replaces wholesale — a
/// half-merged array of tables has no sensible meaning.
fn merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(target), Value::Object(patch)) => {
            for (key, value) in patch {
                merge(target.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (target, patch) => *target = patch.clone(),
    }
}

/// Dotted paths of every leaf that differs between two configs.
fn changed_paths(before: &Config, after: &Config) -> Result<Vec<String>, ConfigStoreError> {
    let to_value = |config: &Config| {
        serde_json::to_value(config)
            .map_err(|err| ConfigStoreError::Invalid(format!("comparing configs: {err}")))
    };
    let mut paths = Vec::new();
    diff(
        &to_value(before)?,
        &to_value(after)?,
        String::new(),
        &mut paths,
    );
    Ok(paths)
}

fn diff(before: &Value, after: &Value, prefix: String, out: &mut Vec<String>) {
    match (before, after) {
        (Value::Object(before), Value::Object(after)) => {
            for key in before.keys().chain(after.keys()) {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                if out.iter().any(|seen| seen == &path) {
                    continue;
                }
                let (lhs, rhs) = (
                    before.get(key).unwrap_or(&Value::Null),
                    after.get(key).unwrap_or(&Value::Null),
                );
                diff(lhs, rhs, path, out);
            }
        }
        (before, after) if before != after => out.push(prefix),
        _ => {}
    }
}

fn is_boot_key(path: &str) -> bool {
    BOOT_KEYS
        .iter()
        .any(|boot| path == *boot || path.starts_with(&format!("{boot}.")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (ConfigStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fastadhunter.toml");
        let config = Config::default();
        config.save(&path).unwrap();
        (ConfigStore::new(config, path), dir)
    }

    #[test]
    fn a_runtime_key_applies_live_without_requiring_a_restart() {
        // `history.retention_days` is a real runtime key: `post_config` pushes
        // it through `apply_history_config` into the writers' shared atomic.
        let (store, _dir) = store();
        let outcome = store
            .apply_patch(&serde_json::json!({"history": {"retention_days": 90}}))
            .unwrap();

        assert_eq!(
            outcome,
            UpdateOutcome {
                applied: true,
                restart_required: false
            }
        );
        assert_eq!(store.current().history.retention_days, 90);
    }

    #[test]
    fn a_boot_key_persists_and_asks_for_a_restart() {
        let (store, _dir) = store();
        let outcome = store
            .apply_patch(&serde_json::json!({"api": {"port": 9443}}))
            .unwrap();

        assert_eq!(
            outcome,
            UpdateOutcome {
                applied: false,
                restart_required: true
            }
        );
        assert_eq!(store.current().api.port, 9443);
    }

    #[test]
    fn the_patch_is_written_back_to_the_toml_file() {
        let (store, _dir) = store();
        store
            .apply_patch(&serde_json::json!({"history": {"retention_days": 30}}))
            .unwrap();

        let written = std::fs::read_to_string(store.path()).unwrap();
        let reparsed = Config::from_toml_str(&written).unwrap();
        assert_eq!(
            reparsed.history.retention_days, 30,
            "the file must reflect the running intent"
        );
    }

    #[test]
    fn merging_leaves_untouched_keys_alone() {
        let (store, _dir) = store();
        let before = store.current();
        store
            .apply_patch(&serde_json::json!({"dns": {"cache": {"max_entries": 123}}}))
            .unwrap();

        let after = store.current();
        assert_eq!(after.dns.cache.max_entries, 123);
        assert_eq!(
            after.dns.cache.max_ttl_seconds,
            before.dns.cache.max_ttl_seconds
        );
        assert_eq!(after.dns.upstreams, before.dns.upstreams);
        assert_eq!(after.api, before.api);
    }

    #[test]
    fn an_unknown_key_is_rejected_and_changes_nothing() {
        let (store, _dir) = store();
        let before = store.current();

        let err = store
            .apply_patch(&serde_json::json!({"dns": {"cache": {"max_entrees": 1}}}))
            .unwrap_err();
        assert!(err.to_string().contains("max_entrees"));

        assert_eq!(store.current().as_ref(), before.as_ref());
        let on_disk =
            Config::from_toml_str(&std::fs::read_to_string(store.path()).unwrap()).unwrap();
        assert_eq!(
            &on_disk,
            before.as_ref(),
            "a rejected patch must not be persisted"
        );
    }

    #[test]
    fn a_semantically_invalid_patch_is_rejected_by_validation() {
        let (store, _dir) = store();
        let err = store
            .apply_patch(&serde_json::json!({"dns": {"listen": {"port": 0}}}))
            .unwrap_err();
        assert!(err.to_string().contains("non-zero port"));
    }

    #[test]
    fn a_no_op_patch_reports_neither_applied_nor_restart_required() {
        // Setting a key to the value it already has changes nothing, so
        // neither flag is raised regardless of the key's class.
        let (store, _dir) = store();
        let outcome = store
            .apply_patch(&serde_json::json!({"dns": {"cache": {"max_entries": 10_000}}}))
            .unwrap();
        assert_eq!(
            outcome,
            UpdateOutcome {
                applied: false,
                restart_required: false
            }
        );
    }

    #[test]
    fn a_mixed_patch_reports_both_flags() {
        let (store, _dir) = store();
        let outcome = store
            .apply_patch(&serde_json::json!({
                "api": {"port": 9443},
                "history": {"retention_days": 60}
            }))
            .unwrap();
        assert_eq!(
            outcome,
            UpdateOutcome {
                applied: true,
                restart_required: true
            }
        );
    }

    /// The classification must track what the code actually *does* with a key,
    /// not what would be convenient. An earlier version of this test asserted
    /// `dns.cache.max_entries` was runtime because CONFIGURATION.md said so —
    /// it passed while `POST /api/v1/config` answered `restart_required: false`
    /// for a value only `DnsCache::new` ever reads, at boot. The runtime list
    /// below is therefore exhaustive, and each entry names its live consumer:
    /// adding to it requires wiring one first.
    #[test]
    fn boot_key_classification_matches_what_actually_applies_the_key() {
        for boot in [
            "engine.mode",
            "runtime.http_runtimes",
            "https.listen.port",
            "https.interception.clients",
            "dns.listen.port",
            "dns.blocking.ttl_seconds",        // Pipeline::new, at boot
            "dns.cache.max_entries",           // DnsCache::new, at boot
            "dns.cache.max_bytes",             // DnsCache::new, at boot
            "dns.upstreams.timeout_ms",        // UpstreamPool::from_config, at boot
            "stats.snapshot_interval_seconds", // Stats::new, at boot
            "history.sample_interval_seconds", // the sampler's interval, at boot
            "api.tls",
            "log.level", // the tracing filter, set once at init
            "log.format",
        ] {
            assert!(
                is_boot_key(boot),
                "{boot} is consumed once at boot — reporting it as applied live would be a lie"
            );
        }
        for (runtime, consumer) in [
            ("history.enabled", "apply_history_config -> writers' atomic"),
            (
                "history.retention_days",
                "apply_history_config -> writers' atomic",
            ),
            (
                "rules.refresh_hours_default",
                "the lists handlers, per request",
            ),
            (
                "schedule.timezone",
                "post_config -> PolicySet::from_config + republish (p2-06)",
            ),
            (
                "policies",
                "the /policies handlers -> set_policies + republish (p2-06)",
            ),
        ] {
            assert!(
                !is_boot_key(runtime),
                "{runtime} is applied live by {consumer} and must stay runtime"
            );
        }
    }

    /// The store still accepts a wholesale list-array replacement, because that
    /// is how `routes::persist_lists` writes the TOML back after a `/lists`
    /// mutation has already been applied to the engine. What is *not* allowed is
    /// reaching this from outside: `POST /api/v1/config` rejects a patch
    /// carrying `rules.lists` with 422, so the engine and the file cannot end up
    /// describing different list sets. This layer is the write-back path, not a
    /// second public entry point.
    #[test]
    fn the_list_write_back_path_replaces_the_array_wholesale() {
        let (store, _dir) = store();
        let outcome = store
            .apply_patch(&serde_json::json!({
                "rules": {"lists": [{"id": "custom", "url": "https://example.org/l.txt"}]}
            }))
            .unwrap();
        assert!(outcome.applied);
        assert!(!outcome.restart_required);

        let lists = &store.current().rules.lists;
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].id, "custom");
    }
}

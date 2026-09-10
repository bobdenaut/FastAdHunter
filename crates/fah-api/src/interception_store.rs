use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use fah_config::{Config, ConfigError};
use fah_model::InterceptionDocument;
use fah_rules::interception::{Active, DocumentError, InterceptionState};

pub const DOCUMENT_FILE: &str = "interception.json";

const STORE_CLOSED: &str = "the certificate store did not open; repair /config and restart";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InterceptionRuntime {
    Live,
    NoListener,
    StoreClosed,
}

#[derive(Debug, thiserror::Error)]
pub enum InterceptionStoreError {
    #[error("{0}")]
    Invalid(DocumentError),
    #[error("{0}")]
    Unavailable(&'static str),
    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: Box<ConfigError>,
    },
    #[error("{path}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: Box<ConfigError>,
    },
}

#[derive(Debug)]
pub struct Loaded {
    pub active: Active,
    pub migrated: bool,
}

#[derive(Debug)]
pub struct Prepared {
    next: Arc<Active>,
    text: String,
}

pub fn load_or_migrate(
    config_dir: &Path,
    config: &mut Config,
    config_path: &Path,
) -> Result<Loaded, InterceptionStoreError> {
    let path = config_dir.join(DOCUMENT_FILE);

    let toml_error = |source: ConfigError| InterceptionStoreError::Toml {
        path: config_path.to_path_buf(),
        source: Box::new(source),
    };
    let text = std::fs::read_to_string(config_path).map_err(|source| {
        toml_error(ConfigError::Io {
            path: config_path.to_path_buf(),
            source,
        })
    })?;
    let mut file = Config::from_toml_str(&text).map_err(toml_error)?;

    let lists = file.https.interception.take();
    let carried = lists.clients.is_some() || lists.exclude_domains.is_some();
    config.https.interception = fah_config::InterceptionConfig::default();

    let active = match std::fs::read_to_string(&path) {
        Ok(text) => {
            let document: InterceptionDocument =
                serde_json::from_str(&text).map_err(|source| InterceptionStoreError::Parse {
                    path: path.clone(),
                    source,
                })?;
            if carried {
                tracing::warn!(
                    document = %path.display(),
                    config = %config_path.display(),
                    "[https.interception] ignored; interception.json is the source of truth"
                );
            }
            Active::compile(document).map_err(InterceptionStoreError::Invalid)?
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            let document = InterceptionDocument {
                clients: lists.clients.unwrap_or_default(),
                exclude_domains: lists.exclude_domains.unwrap_or_default(),
            };
            let active = Active::compile(document).map_err(InterceptionStoreError::Invalid)?;
            let text = document_text(&active.document, &path)?;
            fah_config::write_atomic(&path, &text).map_err(|source| {
                InterceptionStoreError::Write {
                    path: path.clone(),
                    source: Box::new(source),
                }
            })?;
            if carried {
                tracing::info!(
                    document = %path.display(),
                    "migrated [https.interception] into interception.json"
                );
            } else {
                tracing::info!(
                    document = %path.display(),
                    "wrote an empty interception document"
                );
            }
            active
        }
        Err(source) => return Err(InterceptionStoreError::Read { path, source }),
    };

    if carried {
        file.save(config_path).map_err(toml_error)?;
    }

    Ok(Loaded {
        active,
        migrated: carried,
    })
}

fn document_text(
    document: &InterceptionDocument,
    path: &Path,
) -> Result<String, InterceptionStoreError> {
    let mut text =
        serde_json::to_string_pretty(document).map_err(|source| InterceptionStoreError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    text.push('\n');
    Ok(text)
}

pub struct InterceptionStore {
    path: PathBuf,
    state: Arc<InterceptionState>,
    runtime: InterceptionRuntime,
    commit_lock: Mutex<()>,
    #[cfg(any(test, feature = "test-harness"))]
    panic_after_lock: std::sync::atomic::AtomicBool,
}

impl InterceptionStore {
    pub fn new(state: Arc<InterceptionState>, path: PathBuf, runtime: InterceptionRuntime) -> Self {
        Self {
            path,
            state,
            runtime,
            commit_lock: Mutex::new(()),
            #[cfg(any(test, feature = "test-harness"))]
            panic_after_lock: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn current(&self) -> Arc<Active> {
        self.state.current()
    }

    pub fn state(&self) -> &Arc<InterceptionState> {
        &self.state
    }

    pub fn runtime(&self) -> InterceptionRuntime {
        self.runtime
    }

    pub fn prepare(
        &self,
        document: InterceptionDocument,
    ) -> Result<Prepared, InterceptionStoreError> {
        let active = Active::compile(document).map_err(InterceptionStoreError::Invalid)?;
        if self.runtime == InterceptionRuntime::StoreClosed && active.scope.client_count() > 0 {
            return Err(InterceptionStoreError::Unavailable(STORE_CLOSED));
        }
        let text = document_text(&active.document, &self.path)?;
        Ok(Prepared {
            next: Arc::new(active),
            text,
        })
    }

    pub fn commit(&self, prepared: Prepared) -> Result<Arc<Active>, InterceptionStoreError> {
        let Prepared { next, text } = prepared;

        let guard = match self.commit_lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.commit_lock.clear_poison();
                tracing::error!(
                    "a previous interception commit panicked; lock recovered — file and \
                     active state are consistent by construction"
                );
                poisoned.into_inner()
            }
        };

        #[cfg(any(test, feature = "test-harness"))]
        assert!(
            !self
                .panic_after_lock
                .load(std::sync::atomic::Ordering::SeqCst),
            "interception commit panicked after the lock (test hook)"
        );

        fah_config::write_atomic(&self.path, &text).map_err(|source| {
            InterceptionStoreError::Write {
                path: self.path.clone(),
                source: Box::new(source),
            }
        })?;
        self.state.store(Arc::clone(&next));
        drop(guard);

        tracing::info!(
            clients = next.scope.client_count(),
            exclusions = next.scope.exclusion_count(),
            "interception document replaced"
        );
        Ok(next)
    }

    #[cfg(any(test, feature = "test-harness"))]
    pub fn set_panic_after_lock(&self, panic: bool) {
        self.panic_after_lock
            .store(panic, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOML_WITH_LISTS: &str = "[https.interception]\n\
        clients = [\"192.168.88.10\", \"192.168.88.0/24\"]\n\
        exclude_domains = [\"bank.example\"]\n";

    struct Boot {
        _dir: tempfile::TempDir,
        config_dir: PathBuf,
        config_path: PathBuf,
        document_path: PathBuf,
    }

    fn boot(toml: &str) -> Boot {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().to_path_buf();
        let config_path = config_dir.join("fastadhunter.toml");
        std::fs::write(&config_path, toml).unwrap();
        let document_path = config_dir.join(DOCUMENT_FILE);
        Boot {
            _dir: dir,
            config_dir,
            config_path,
            document_path,
        }
    }

    impl Boot {
        fn run(&self, config: &mut Config) -> Result<Loaded, InterceptionStoreError> {
            load_or_migrate(&self.config_dir, config, &self.config_path)
        }

        fn document(&self) -> String {
            std::fs::read_to_string(&self.document_path).unwrap()
        }

        fn toml(&self) -> String {
            std::fs::read_to_string(&self.config_path).unwrap()
        }
    }

    fn effective(toml: &str) -> Config {
        Config::from_toml_str(toml).unwrap()
    }

    fn document(clients: &[&str], exclude_domains: &[&str]) -> InterceptionDocument {
        InterceptionDocument {
            clients: clients.iter().map(|entry| entry.to_string()).collect(),
            exclude_domains: exclude_domains
                .iter()
                .map(|entry| entry.to_string())
                .collect(),
        }
    }

    fn store(runtime: InterceptionRuntime) -> (tempfile::TempDir, InterceptionStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DOCUMENT_FILE);
        let state = Arc::new(InterceptionState::default());
        (dir, InterceptionStore::new(state, path, runtime))
    }

    fn live() -> (tempfile::TempDir, InterceptionStore) {
        store(InterceptionRuntime::Live)
    }

    fn commit(store: &InterceptionStore, document: InterceptionDocument) -> Arc<Active> {
        let prepared = store.prepare(document).unwrap();
        store.commit(prepared).unwrap()
    }

    #[test]
    fn first_boot_migrates_toml_lists_and_clears_them() {
        let boot = boot(TOML_WITH_LISTS);
        let mut config = effective(TOML_WITH_LISTS);

        let loaded = boot.run(&mut config).unwrap();

        assert!(loaded.migrated);
        assert_eq!(
            loaded.active.document,
            document(&["192.168.88.10", "192.168.88.0/24"], &["bank.example"])
        );
        assert!(config.https.interception.is_absent());

        let stored: InterceptionDocument = serde_json::from_str(&boot.document()).unwrap();
        assert_eq!(stored, loaded.active.document);
        assert!(boot.document().ends_with("\n"));

        let toml = boot.toml();
        assert!(!toml.contains("interception"), "{toml}");
        assert!(!toml.contains("192.168.88.10"), "{toml}");
    }

    #[test]
    fn migration_saves_the_file_layer_not_the_effective_config() {
        let file_toml =
            format!("{TOML_WITH_LISTS}\n[api]\nport = 8443\n\n[log]\nlevel = \"info\"\n");
        let boot = boot(&file_toml);

        let mut config = effective(&file_toml);
        config.api.port = 9443;
        config.log.level = fah_config::LogLevel::Debug;
        let before = config.clone();

        boot.run(&mut config).unwrap();

        let saved = Config::from_toml_str(&boot.toml()).unwrap();
        assert!(saved.https.interception.is_absent());
        assert_eq!(saved.api.port, 8443);
        assert_eq!(saved.log.level, fah_config::LogLevel::Info);

        assert_eq!(config.api.port, 9443);
        assert_eq!(config.log.level, fah_config::LogLevel::Debug);
        let mut expected = before;
        expected.https.interception = fah_config::InterceptionConfig::default();
        assert_eq!(config, expected);
    }

    #[test]
    fn migration_fails_boot_when_the_toml_cannot_be_reread() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().to_path_buf();
        let config_path = config_dir.join("fastadhunter.toml");
        std::fs::create_dir(&config_path).unwrap();

        let mut config = Config::default();
        let error = load_or_migrate(&config_dir, &mut config, &config_path).unwrap_err();

        assert!(
            matches!(error, InterceptionStoreError::Toml { .. }),
            "{error:?}"
        );
        assert!(!config_dir.join(DOCUMENT_FILE).exists());
    }

    #[test]
    fn an_upgrade_with_literal_empty_lists_migrates_to_an_empty_document() {
        let toml = "[https.interception]\nclients = []\n";
        let boot = boot(toml);
        let mut config = effective(toml);

        let loaded = boot.run(&mut config).unwrap();

        assert!(loaded.migrated);
        assert_eq!(loaded.active.document, InterceptionDocument::default());
        assert_eq!(loaded.active.scope.client_count(), 0);
        assert!(!boot.toml().contains("interception"));
    }

    #[test]
    fn a_fresh_install_writes_an_empty_document_and_leaves_the_toml_alone() {
        let toml = "[api]\nport = 8443\n";
        let boot = boot(toml);
        let mut config = effective(toml);

        let loaded = boot.run(&mut config).unwrap();

        assert!(!loaded.migrated);
        assert_eq!(loaded.active.document, InterceptionDocument::default());
        assert_eq!(boot.toml(), toml);
        assert_eq!(
            boot.document(),
            "{\n  \"clients\": [],\n  \"exclude_domains\": []\n}\n"
        );
    }

    #[test]
    fn an_existing_document_wins_and_is_byte_identical_after_boot() {
        let boot = boot(TOML_WITH_LISTS);
        let existing = "{\n  \"clients\": [\"10.0.0.7\"],\n  \"exclude_domains\": []\n}\n";
        std::fs::write(&boot.document_path, existing).unwrap();
        let mut config = effective(TOML_WITH_LISTS);

        let loaded = boot.run(&mut config).unwrap();

        assert_eq!(loaded.active.document, document(&["10.0.0.7"], &[]));
        assert_eq!(boot.document(), existing);
        assert!(!boot.toml().contains("interception"));
    }

    #[test]
    fn an_unreadable_document_fails_boot_without_writing() {
        let boot = boot(TOML_WITH_LISTS);
        std::fs::create_dir(&boot.document_path).unwrap();
        let mut config = effective(TOML_WITH_LISTS);

        let error = boot.run(&mut config).unwrap_err();

        assert!(
            matches!(error, InterceptionStoreError::Read { .. }),
            "{error:?}"
        );
        assert!(boot.document_path.is_dir());
        assert!(boot.toml().contains("interception"));
    }

    #[test]
    fn a_second_boot_touches_nothing() {
        let boot = boot(TOML_WITH_LISTS);
        let mut config = effective(TOML_WITH_LISTS);
        boot.run(&mut config).unwrap();

        let document_before = boot.document();
        let toml_before = boot.toml();

        let mut config = Config::from_toml_str(&toml_before).unwrap();
        let loaded = boot.run(&mut config).unwrap();

        assert!(!loaded.migrated);
        assert_eq!(boot.document(), document_before);
        assert_eq!(boot.toml(), toml_before);
    }

    #[test]
    fn a_malformed_document_fails_boot_naming_the_file() {
        let boot = boot("[api]\nport = 8443\n");
        std::fs::write(&boot.document_path, "{not json").unwrap();
        let mut config = Config::default();

        let error = boot.run(&mut config).unwrap_err();

        assert!(
            matches!(&error, InterceptionStoreError::Parse { path, .. } if path == &boot.document_path),
            "{error:?}"
        );
        assert!(error.to_string().contains(DOCUMENT_FILE), "{error}");
    }

    #[test]
    fn an_unknown_key_in_the_document_fails_boot() {
        let boot = boot("[api]\nport = 8443\n");
        std::fs::write(&boot.document_path, r#"{"client": []}"#).unwrap();
        let mut config = Config::default();

        let error = boot.run(&mut config).unwrap_err();

        assert!(
            matches!(error, InterceptionStoreError::Parse { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn an_over_cap_document_fails_boot() {
        let boot = boot("[api]\nport = 8443\n");
        let clients: Vec<String> = (0..257)
            .map(|index| format!("10.0.{}.{}", index / 256, index % 256))
            .collect();
        std::fs::write(
            &boot.document_path,
            serde_json::to_string(&InterceptionDocument {
                clients,
                exclude_domains: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();
        let mut config = Config::default();

        let error = boot.run(&mut config).unwrap_err();

        assert!(
            matches!(error, InterceptionStoreError::Invalid(_)),
            "{error:?}"
        );
    }

    #[test]
    fn invalid_toml_values_fail_boot_before_the_document_is_written() {
        let toml = "[https.interception]\nclients = [\"10.0.0.300\"]\n";
        let boot = boot(toml);
        let mut config = effective(toml);

        let error = boot.run(&mut config).unwrap_err();

        assert!(
            matches!(error, InterceptionStoreError::Invalid(_)),
            "{error:?}"
        );
        assert!(!boot.document_path.exists());
        assert_eq!(boot.toml(), toml);
    }

    #[test]
    fn commit_persists_then_publishes_one_arc() {
        let (_dir, store) = live();
        let active = commit(&store, document(&["192.168.88.10"], &["bank.example"]));

        let stored: InterceptionDocument =
            serde_json::from_str(&std::fs::read_to_string(&store.path).unwrap()).unwrap();
        assert_eq!(stored, active.document);
        assert_eq!(active.scope.client_count(), 1);
        assert_eq!(active.scope.exclusion_count(), 1);
        assert!(Arc::ptr_eq(&store.current(), &active));
        assert!(Arc::ptr_eq(&store.state().current(), &active));
    }

    #[test]
    fn prepare_rejects_without_side_effects() {
        let (_dir, store) = live();
        let before = store.current();

        let over_cap = InterceptionDocument {
            clients: (0..257)
                .map(|index| format!("10.0.{}.{}", index / 256, index % 256))
                .collect(),
            exclude_domains: Vec::new(),
        };
        for bad in [
            over_cap,
            document(&["10.0.0.1", "10.0.0.1"], &[]),
            document(&["10.0.0.300"], &[]),
        ] {
            assert!(matches!(
                store.prepare(bad),
                Err(InterceptionStoreError::Invalid(_))
            ));
            assert!(!store.path.exists());
            assert!(Arc::ptr_eq(&store.current(), &before));
        }
    }

    #[test]
    fn store_closed_rejects_listing_a_client_but_accepts_an_empty_list() {
        let (_dir, store) = store(InterceptionRuntime::StoreClosed);

        let error = store.prepare(document(&["10.0.0.1"], &[])).unwrap_err();
        assert!(
            matches!(error, InterceptionStoreError::Unavailable(STORE_CLOSED)),
            "{error:?}"
        );
        assert!(!store.path.exists());

        let active = commit(&store, document(&[], &["bank.example"]));
        assert_eq!(active.scope.exclusion_count(), 1);
    }

    #[test]
    fn no_listener_stores_the_document() {
        let (_dir, store) = store(InterceptionRuntime::NoListener);
        let active = commit(&store, document(&["10.0.0.1"], &[]));
        assert_eq!(active.scope.client_count(), 1);
        assert!(store.path.exists());
    }

    #[test]
    fn a_write_failure_publishes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "not a directory").unwrap();
        let store = InterceptionStore::new(
            Arc::new(InterceptionState::default()),
            blocker.join(DOCUMENT_FILE),
            InterceptionRuntime::Live,
        );
        let before = store.current();

        let prepared = store.prepare(document(&["10.0.0.1"], &[])).unwrap();
        let error = store.commit(prepared).unwrap_err();

        assert!(
            matches!(error, InterceptionStoreError::Write { .. }),
            "{error:?}"
        );
        assert!(Arc::ptr_eq(&store.current(), &before));
    }

    #[test]
    fn two_commits_serialize_and_the_last_one_wins_on_file_and_runtime_together() {
        let (_dir, store) = live();
        let store = Arc::new(store);

        let mut handles = Vec::new();
        for index in 0..2u8 {
            let store = Arc::clone(&store);
            handles.push(std::thread::spawn(move || {
                for round in 0..16 {
                    let entry = format!("10.0.{index}.{round}");
                    let prepared = store.prepare(document(&[&entry], &[])).unwrap();
                    store.commit(prepared).unwrap();
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }

        let stored: InterceptionDocument =
            serde_json::from_str(&std::fs::read_to_string(&store.path).unwrap()).unwrap();
        assert_eq!(stored, store.current().document);
    }

    #[test]
    fn a_poisoned_lock_is_recovered_cleared_and_the_commit_proceeds() {
        let (_dir, store) = live();
        let store = Arc::new(store);

        let poisoner = Arc::clone(&store);
        std::thread::spawn(move || {
            let _guard = poisoner.commit_lock.lock().unwrap();
            panic!("poison the commit lock");
        })
        .join()
        .expect_err("the poisoning thread must panic");
        assert!(store.commit_lock.is_poisoned());

        let active = commit(&store, document(&["10.0.0.5"], &[]));

        assert!(!store.commit_lock.is_poisoned());
        let stored: InterceptionDocument =
            serde_json::from_str(&std::fs::read_to_string(&store.path).unwrap()).unwrap();
        assert_eq!(stored, active.document);
        assert!(Arc::ptr_eq(&store.current(), &active));
    }

    #[test]
    fn a_recovered_commit_publishes_only_what_it_prepared() {
        let (_dir, store) = live();
        let store = Arc::new(store);
        commit(&store, document(&["10.0.0.1"], &[]));

        let poisoner = Arc::clone(&store);
        std::thread::spawn(move || {
            let _guard = poisoner.commit_lock.lock().unwrap();
            panic!("poison the commit lock");
        })
        .join()
        .expect_err("the poisoning thread must panic");

        let prepared = store.prepare(document(&["10.0.0.9"], &[])).unwrap();
        let published = store.commit(prepared).unwrap();

        assert_eq!(published.document, document(&["10.0.0.9"], &[]));
        assert!(Arc::ptr_eq(&store.current(), &published));
    }

    #[test]
    fn a_commit_that_panics_after_the_lock_leaves_file_and_active_unchanged() {
        let (_dir, store) = live();
        let store = Arc::new(store);
        let first = commit(&store, document(&["10.0.0.1"], &[]));
        let bytes = std::fs::read_to_string(&store.path).unwrap();

        store.set_panic_after_lock(true);
        let prepared = store.prepare(document(&["10.0.0.2"], &[])).unwrap();
        let panicking = Arc::clone(&store);
        std::thread::spawn(move || panicking.commit(prepared))
            .join()
            .expect_err("the hook must panic inside commit");

        assert_eq!(std::fs::read_to_string(&store.path).unwrap(), bytes);
        assert!(Arc::ptr_eq(&store.current(), &first));
        assert!(store.commit_lock.is_poisoned());

        store.set_panic_after_lock(false);
        let next = commit(&store, document(&["10.0.0.2"], &[]));
        assert!(!store.commit_lock.is_poisoned());
        assert_eq!(next.document, document(&["10.0.0.2"], &[]));
    }
}

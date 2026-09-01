use std::borrow::Cow;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rustls::pki_types::CertificateDer;
use rustls::sign::CertifiedKey;

use crate::ca::{self, CaHandle, CaParams, CaSummary};
use crate::error::CertError;
use crate::import::{ValidatedCaPair, ValidatedServerPair};
use crate::leaf::{LeafCache, LeafCacheStats, LEAF_CACHE_CAPACITY};

pub(crate) const API_CERT_FILE: &str = "api-cert.pem";
pub(crate) const API_KEY_FILE: &str = "api-key.pem";
pub(crate) const API_CERT_TMP_FILE: &str = "api-cert.pem.tmp";
pub(crate) const API_KEY_TMP_FILE: &str = "api-key.pem.tmp";
pub(crate) const API_SOURCE_FILE: &str = "api-cert.source";
pub(crate) const API_ARCHIVE_DIR: &str = "api-archive";

pub(crate) const CA_CERT_FILE: &str = "ca-cert.pem";
pub(crate) const CA_KEY_FILE: &str = "ca-key.pem";
pub(crate) const CA_CERT_TMP_FILE: &str = "ca-cert.pem.tmp";
pub(crate) const CA_KEY_TMP_FILE: &str = "ca-key.pem.tmp";
pub(crate) const CA_ARCHIVE_DIR: &str = "ca-archive";

const ARCHIVE_ATTEMPTS: u32 = 1024;
pub const MAX_ARCHIVES: usize = 8;
const MAX_HOST_LEN: usize = 253;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiPairSource {
    SelfSigned,
    Imported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertStatus {
    pub ca: Option<CaSummary>,
    pub leaves: LeafCacheStats,
    pub api_pair: ApiPairSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaInstalled {
    pub summary: CaSummary,
    pub archived_previous: bool,
}

pub(crate) struct PairPaths {
    pub(crate) cert: PathBuf,
    pub(crate) key: PathBuf,
    pub(crate) cert_tmp: PathBuf,
    pub(crate) key_tmp: PathBuf,
}

impl PairPaths {
    fn new(config_dir: &Path, cert: &str, key: &str, cert_tmp: &str, key_tmp: &str) -> Self {
        Self {
            cert: config_dir.join(cert),
            key: config_dir.join(key),
            cert_tmp: config_dir.join(cert_tmp),
            key_tmp: config_dir.join(key_tmp),
        }
    }

    pub(crate) fn api(config_dir: &Path) -> Self {
        Self::new(
            config_dir,
            API_CERT_FILE,
            API_KEY_FILE,
            API_CERT_TMP_FILE,
            API_KEY_TMP_FILE,
        )
    }

    pub(crate) fn ca(config_dir: &Path) -> Self {
        Self::new(
            config_dir,
            CA_CERT_FILE,
            CA_KEY_FILE,
            CA_CERT_TMP_FILE,
            CA_KEY_TMP_FILE,
        )
    }
}

pub(crate) fn load_pair(paths: &PairPaths) -> Result<Option<(String, String)>, CertError> {
    if paths.cert.exists() && !paths.key.exists() && paths.key_tmp.exists() {
        tracing::warn!(
            key = %paths.key.display(),
            "completing an interrupted certificate generation"
        );
        rename(&paths.key_tmp, &paths.key)?;
    }

    match (paths.cert.exists(), paths.key.exists()) {
        (true, true) => Ok(Some((read(&paths.cert)?, read(&paths.key)?))),
        (true, false) => Err(CertError::IncompletePair {
            present: paths.cert.clone(),
            missing: paths.key.clone(),
        }),
        (false, true) => Err(CertError::IncompletePair {
            present: paths.key.clone(),
            missing: paths.cert.clone(),
        }),
        (false, false) => Ok(None),
    }
}

pub(crate) fn discard_staged(paths: &PairPaths) {
    discard(&paths.cert_tmp);
    discard(&paths.key_tmp);
}

pub(crate) fn stage_pair(
    paths: &PairPaths,
    cert_pem: &str,
    key_pem: &str,
) -> Result<(), CertError> {
    discard(&paths.key_tmp);
    let staged =
        write_private(&paths.key_tmp, key_pem).and_then(|()| write(&paths.cert_tmp, cert_pem));
    if let Err(error) = staged {
        discard(&paths.cert_tmp);
        discard(&paths.key_tmp);
        return Err(error);
    }
    Ok(())
}

pub(crate) fn commit_pair(paths: &PairPaths) -> Result<(), CertError> {
    if let Err(error) = rename(&paths.cert_tmp, &paths.cert) {
        discard(&paths.cert_tmp);
        discard(&paths.key_tmp);
        return Err(error);
    }
    if let Err(error) = rename(&paths.key_tmp, &paths.key) {
        discard(&paths.cert);
        discard(&paths.key_tmp);
        return Err(error);
    }
    Ok(())
}

pub(crate) fn write_pair(
    paths: &PairPaths,
    cert_pem: &str,
    key_pem: &str,
) -> Result<(), CertError> {
    stage_pair(paths, cert_pem, key_pem)?;
    commit_pair(paths)
}

pub(crate) fn read(path: &Path) -> Result<String, CertError> {
    fs::read_to_string(path).map_err(|source| CertError::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn write(path: &Path, text: &str) -> Result<(), CertError> {
    ensure_parent(path)?;
    fs::write(path, text).map_err(|source| CertError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
pub(crate) fn write_private(path: &Path, text: &str) -> Result<(), CertError> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt;

    ensure_parent(path)?;
    let io = |source| CertError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(io)?;
    file.write_all(text.as_bytes()).map_err(io)
}

#[cfg(not(unix))]
pub(crate) fn write_private(path: &Path, text: &str) -> Result<(), CertError> {
    write(path, text)
}

fn ensure_parent(path: &Path) -> Result<(), CertError> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            fs::create_dir_all(parent).map_err(|source| CertError::Io {
                path: parent.to_path_buf(),
                source,
            })
        }
        _ => Ok(()),
    }
}

pub(crate) fn rename(from: &Path, to: &Path) -> Result<(), CertError> {
    fs::rename(from, to).map_err(|source| CertError::Io {
        path: to.to_path_buf(),
        source,
    })
}

pub(crate) fn copy(from: &Path, to: &Path) -> Result<(), CertError> {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|source| CertError::Io {
            path: to.to_path_buf(),
            source,
        })
}

pub(crate) fn discard(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(unix)]
pub(crate) fn restrict_permissions(path: &Path) -> Result<(), CertError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| CertError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
pub(crate) fn restrict_permissions(_path: &Path) -> Result<(), CertError> {
    Ok(())
}

pub(crate) fn all_certificates(
    text: &str,
    what: &'static str,
) -> Result<Vec<CertificateDer<'static>>, CertError> {
    let mut reader = text.as_bytes();
    let certificates = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CertError::parse(what, error))?;
    if certificates.is_empty() {
        return Err(CertError::parse(what, "no CERTIFICATE block"));
    }
    Ok(certificates)
}

pub(crate) fn first_certificate(
    text: &str,
    what: &'static str,
) -> Result<CertificateDer<'static>, CertError> {
    all_certificates(text, what).map(|mut certificates| certificates.swap_remove(0))
}

pub(crate) fn certificate_pem(certificates: &[CertificateDer<'_>]) -> String {
    const CONFIG: pem::EncodeConfig = pem::EncodeConfig::new().set_line_ending(pem::LineEnding::LF);
    certificates
        .iter()
        .map(|der| pem::encode_config(&pem::Pem::new("CERTIFICATE", der.as_ref().to_vec()), CONFIG))
        .collect()
}

pub struct CertStore {
    config_dir: PathBuf,
    ca: Mutex<Option<Arc<CaHandle>>>,
    leaves: LeafCache,
    api: Mutex<()>,
    api_imported: AtomicBool,
}

impl fmt::Debug for CertStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CertStore")
            .field("config_dir", &self.config_dir)
            .field("ca", &self.ca_summary())
            .field("leaves", &self.leaves.stats())
            .finish()
    }
}

impl CertStore {
    pub fn open(config_dir: &Path) -> Result<Self, CertError> {
        let paths = PairPaths::ca(config_dir);
        let handle = match load_pair(&paths)? {
            Some((cert_pem, key_pem)) => {
                let handle = Arc::new(CaHandle::load(&cert_pem, &key_pem)?);
                discard_staged(&paths);
                Some(handle)
            }
            None => None,
        };
        Ok(Self {
            config_dir: config_dir.to_path_buf(),
            ca: Mutex::new(handle),
            leaves: LeafCache::with_capacity(LEAF_CACHE_CAPACITY),
            api: Mutex::new(()),
            api_imported: AtomicBool::new(read_api_source(config_dir)),
        })
    }

    pub fn has_ca(&self) -> bool {
        self.ca().is_some()
    }

    pub fn ca_summary(&self) -> Option<CaSummary> {
        self.ca().map(|ca| ca.summary().clone())
    }

    pub fn generate_ca(&self, params: &CaParams) -> Result<CaInstalled, CertError> {
        let generated = ca::generate(params)?;
        self.install_ca(&generated.cert_pem, &generated.key_pem)
    }

    pub fn install_ca_pair(&self, pair: &ValidatedCaPair) -> Result<CaInstalled, CertError> {
        self.install_ca(pair.cert_pem(), pair.key_pem())
    }

    fn install_ca(&self, cert_pem: &str, key_pem: &str) -> Result<CaInstalled, CertError> {
        let handle = Arc::new(CaHandle::load(cert_pem, key_pem)?);

        let mut slot = self.lock_ca();
        let paths = PairPaths::ca(&self.config_dir);
        stage_pair(&paths, handle.cert_pem(), key_pem)?;

        let archive = match paths.cert.exists() || paths.key.exists() {
            true => match self.archive_existing_ca(&paths) {
                Ok(archive) => Some(archive),
                Err(error) => {
                    discard(&paths.cert_tmp);
                    discard(&paths.key_tmp);
                    return Err(error);
                }
            },
            false => None,
        };

        if let Err(error) = commit_pair(&paths) {
            if let Some(archive) = &archive {
                restore_ca(archive, &paths);
            }
            return Err(error);
        }

        if let Some(archive) = &archive {
            tracing::warn!(
                archive = %archive.display(),
                "replacing the certificate authority; every client trusting the old root \
                 must install the new one"
            );
        }

        let summary = handle.summary().clone();
        *slot = Some(handle);
        self.leaves.clear();
        drop(slot);
        Ok(CaInstalled {
            summary,
            archived_previous: archive.is_some(),
        })
    }

    fn archive_existing_ca(&self, paths: &PairPaths) -> Result<PathBuf, CertError> {
        self.archive_pair(paths, CA_ARCHIVE_DIR, CA_CERT_FILE, CA_KEY_FILE)
    }

    fn archive_pair(
        &self,
        paths: &PairPaths,
        archive_dir: &'static str,
        cert_file: &str,
        key_file: &str,
    ) -> Result<PathBuf, CertError> {
        let dir = self.unused_archive_dir(archive_dir)?;
        if let Err(error) = fill_archive(&dir, paths, cert_file, key_file) {
            let _ = fs::remove_dir_all(&dir);
            return Err(error);
        }
        Ok(dir)
    }

    fn unused_archive_dir(&self, archive_dir: &'static str) -> Result<PathBuf, CertError> {
        let base = self.config_dir.join(archive_dir);
        fs::create_dir_all(&base).map_err(|source| CertError::Io {
            path: base.clone(),
            source,
        })?;
        let retired = fs::read_dir(&base)
            .map_err(|source| CertError::Io {
                path: base.clone(),
                source,
            })?
            .count();
        if retired >= MAX_ARCHIVES {
            return Err(CertError::ArchiveFull {
                archive: archive_dir,
                limit: MAX_ARCHIVES,
            });
        }

        let stamp = unix_now();
        for attempt in 0..ARCHIVE_ATTEMPTS {
            let dir = match attempt {
                0 => base.join(stamp.to_string()),
                _ => base.join(format!("{stamp}-{attempt}")),
            };
            match fs::create_dir(&dir) {
                Ok(()) => return Ok(dir),
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(source) => return Err(CertError::Io { path: dir, source }),
            }
        }
        Err(CertError::Io {
            path: base,
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "no unused archive directory for this second",
            ),
        })
    }

    pub fn prewarm(&self, host: &str) -> Result<Arc<CertifiedKey>, CertError> {
        let host = normalize(host).ok_or(CertError::InvalidHost)?;

        loop {
            let (ca, epoch) = {
                let slot = self.lock_ca();
                let ca = slot.clone().ok_or(CertError::NoCa)?;
                (ca, self.leaves.epoch())
            };
            let now = unix_now();
            let summary = ca.summary();
            if now < summary.not_before {
                return Err(CertError::NotYetValid {
                    not_before: summary.not_before,
                });
            }
            if now > summary.not_after {
                return Err(CertError::Expired {
                    not_after: summary.not_after,
                });
            }

            if let Some(key) = self.leaves.prewarm(&ca, host.as_ref(), now, epoch)? {
                return Ok(key);
            }
        }
    }

    pub fn cached_leaf(&self, host: &str) -> Option<Arc<CertifiedKey>> {
        let host = normalize(host)?;
        self.leaves.cached(host.as_ref(), unix_now())
    }

    pub fn leaf_cache_stats(&self) -> LeafCacheStats {
        self.leaves.stats()
    }

    pub fn ca_public_pem(&self) -> Option<String> {
        self.ca().map(|ca| ca.cert_pem().to_string())
    }

    pub fn ca_public_der(&self) -> Option<Vec<u8>> {
        self.ca().map(|ca| ca.cert_der().as_ref().to_vec())
    }

    pub fn install_api_pair(&self, pair: &ValidatedServerPair) -> Result<(), CertError> {
        let fingerprint = ca::fingerprint(&first_certificate(pair.cert_pem(), "the certificate")?);

        let _serialized = self.lock_api();
        let paths = PairPaths::api(&self.config_dir);
        let marker = self.config_dir.join(API_SOURCE_FILE);
        stage_pair(&paths, pair.cert_pem(), pair.key_pem())?;

        let archive = match paths.cert.exists() || paths.key.exists() {
            true => match self.archive_pair(&paths, API_ARCHIVE_DIR, API_CERT_FILE, API_KEY_FILE) {
                Ok(archive) => Some(archive),
                Err(error) => {
                    discard_staged(&paths);
                    return Err(error);
                }
            },
            false => None,
        };

        let previous_marker = read(&marker).ok();
        if let Err(error) = write(&marker, &fingerprint) {
            discard_staged(&paths);
            return Err(error);
        }

        if let Err(error) = commit_pair(&paths) {
            if let Some(archive) = &archive {
                restore_pair(archive, &paths, API_CERT_FILE, API_KEY_FILE);
            }
            match &previous_marker {
                Some(previous) => {
                    let _ = write(&marker, previous);
                }
                None => discard(&marker),
            }
            return Err(error);
        }

        self.api_imported.store(true, Ordering::Relaxed);
        if let Some(archive) = &archive {
            tracing::info!(
                archive = %archive.display(),
                "replaced the API server certificate; the previous pair is archived and \
                 the new one applies at the next restart"
            );
        }
        Ok(())
    }

    pub fn api_certified_key(&self) -> Result<Arc<CertifiedKey>, CertError> {
        let _serialized = self.lock_api();
        crate::api::certified_key(&self.config_dir)
    }

    pub fn api_pair_source(&self) -> ApiPairSource {
        match self.api_imported.load(Ordering::Relaxed) {
            true => ApiPairSource::Imported,
            false => ApiPairSource::SelfSigned,
        }
    }

    pub fn status(&self) -> CertStatus {
        CertStatus {
            ca: self.ca_summary(),
            leaves: self.leaves.stats(),
            api_pair: self.api_pair_source(),
        }
    }

    fn ca(&self) -> Option<Arc<CaHandle>> {
        self.lock_ca().clone()
    }

    fn lock_ca(&self) -> MutexGuard<'_, Option<Arc<CaHandle>>> {
        self.ca.lock().unwrap_or_else(|poisoned| {
            self.ca.clear_poison();
            poisoned.into_inner()
        })
    }

    fn lock_api(&self) -> MutexGuard<'_, ()> {
        self.api.lock().unwrap_or_else(|poisoned| {
            self.api.clear_poison();
            poisoned.into_inner()
        })
    }
}

fn normalize(host: &str) -> Option<Cow<'_, str>> {
    if host.is_empty() || host.len() > MAX_HOST_LEN {
        return None;
    }
    Some(match host.bytes().any(|byte| byte.is_ascii_uppercase()) {
        true => Cow::Owned(host.to_ascii_lowercase()),
        false => Cow::Borrowed(host),
    })
}

fn fill_archive(
    dir: &Path,
    paths: &PairPaths,
    cert_file: &str,
    key_file: &str,
) -> Result<(), CertError> {
    for (from, name) in [(&paths.cert, cert_file), (&paths.key, key_file)] {
        if from.exists() {
            copy(from, &dir.join(name))?;
        }
    }
    let key = dir.join(key_file);
    match key.exists() {
        true => restrict_permissions(&key),
        false => Ok(()),
    }
}

fn restore_ca(archive: &Path, paths: &PairPaths) {
    restore_pair(archive, paths, CA_CERT_FILE, CA_KEY_FILE);
}

fn restore_pair(archive: &Path, paths: &PairPaths, cert_file: &str, key_file: &str) {
    discard(&paths.cert_tmp);
    discard(&paths.key_tmp);
    for (name, to) in [(cert_file, &paths.cert), (key_file, &paths.key)] {
        let from = archive.join(name);
        if from.exists() {
            if let Err(error) = copy(&from, to) {
                tracing::error!(
                    %error,
                    path = %to.display(),
                    "restoring the archived certificate pair failed; \
                     it is preserved under the archive directory"
                );
            }
        }
    }
}

pub(crate) fn read_api_source(config_dir: &Path) -> bool {
    let Ok(marker) = read(&config_dir.join(API_SOURCE_FILE)) else {
        return false;
    };
    let Ok(cert_pem) = read(&config_dir.join(API_CERT_FILE)) else {
        return false;
    };
    let Ok(live) = first_certificate(&cert_pem, "the certificate") else {
        return false;
    };
    marker.trim() == ca::fingerprint(&live)
}

pub(crate) fn clear_api_source(config_dir: &Path) {
    discard(&config_dir.join(API_SOURCE_FILE));
}

fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_secs() as i64,
        Err(before) => -(before.duration().as_secs() as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{validate_ca_pair, validate_server_pair};

    fn store() -> (tempfile::TempDir, CertStore) {
        crate::install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        let store = CertStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn a_config_directory_without_an_authority_is_valid() {
        let (_dir, store) = store();
        assert!(!store.has_ca());
        assert!(store.ca_summary().is_none());
        assert!(store.ca_public_pem().is_none());
        assert!(store.ca_public_der().is_none());
        assert!(matches!(store.prewarm("a.example"), Err(CertError::NoCa)));
    }

    #[test]
    fn a_generated_authority_survives_a_reopen_unchanged() {
        let (dir, store) = store();
        let installed = store.generate_ca(&CaParams::default()).unwrap();
        assert!(
            !installed.archived_previous,
            "a first generation archives nothing"
        );
        let summary = installed.summary;
        assert!(dir.path().join(CA_CERT_FILE).exists());
        assert!(dir.path().join(CA_KEY_FILE).exists());

        let reopened = CertStore::open(dir.path()).unwrap();
        assert_eq!(reopened.ca_summary(), Some(summary));
    }

    #[test]
    fn regeneration_archives_the_old_pair_and_never_deletes_a_private_key() {
        let (dir, store) = store();
        let first = store.generate_ca(&CaParams::default()).unwrap();
        let old_key = fs::read_to_string(dir.path().join(CA_KEY_FILE)).unwrap();

        let second = store.generate_ca(&CaParams::default()).unwrap();
        assert!(
            second.archived_previous,
            "a replacement reports the archive it made"
        );
        assert_ne!(
            first.summary.fingerprint_sha256,
            second.summary.fingerprint_sha256
        );

        let archive = dir.path().join(CA_ARCHIVE_DIR);
        let stamped = fs::read_dir(&archive).unwrap().next().unwrap().unwrap();
        assert_eq!(
            fs::read_to_string(stamped.path().join(CA_KEY_FILE)).unwrap(),
            old_key
        );
        assert!(stamped.path().join(CA_CERT_FILE).exists());
    }

    #[test]
    fn regeneration_purges_leaves_signed_by_the_archived_authority() {
        let (_dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();
        let before = store.prewarm("a.example").unwrap();
        assert_eq!(store.leaf_cache_stats().size, 1);

        store.generate_ca(&CaParams::default()).unwrap();
        assert_eq!(store.leaf_cache_stats().size, 0);

        let after = store.prewarm("a.example").unwrap();
        assert_ne!(before.cert[0], after.cert[0]);
    }

    #[test]
    fn a_half_authority_pair_is_reported_rather_than_silently_regenerated() {
        let (dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();
        fs::remove_file(dir.path().join(CA_KEY_FILE)).unwrap();

        assert!(matches!(
            CertStore::open(dir.path()),
            Err(CertError::IncompletePair { .. })
        ));
    }

    #[test]
    fn an_interrupted_authority_generation_is_completed_on_the_next_open() {
        let (dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();
        let key = fs::read_to_string(dir.path().join(CA_KEY_FILE)).unwrap();
        fs::rename(
            dir.path().join(CA_KEY_FILE),
            dir.path().join(CA_KEY_TMP_FILE),
        )
        .unwrap();

        let reopened = CertStore::open(dir.path()).unwrap();
        assert!(reopened.has_ca());
        assert_eq!(
            fs::read_to_string(dir.path().join(CA_KEY_FILE)).unwrap(),
            key
        );
        assert!(!dir.path().join(CA_KEY_TMP_FILE).exists());
    }

    #[test]
    fn an_imported_api_pair_is_persisted_and_marked() {
        let (dir, store) = store();
        assert_eq!(store.api_pair_source(), ApiPairSource::SelfSigned);

        let generated = ca::generate(&CaParams::default()).unwrap();
        let pair = validate_server_pair(&generated.cert_pem, &generated.key_pem).unwrap();
        store.install_api_pair(&pair).unwrap();

        let written = fs::read_to_string(dir.path().join(API_CERT_FILE)).unwrap();
        assert_eq!(
            all_certificates(&written, "the certificate").unwrap(),
            all_certificates(&generated.cert_pem, "the certificate").unwrap()
        );
        assert_eq!(store.api_pair_source(), ApiPairSource::Imported);
        assert_eq!(store.status().api_pair, ApiPairSource::Imported);
    }

    #[test]
    fn replacing_the_api_pair_archives_the_one_it_replaces() {
        let (dir, store) = store();
        let first = ca::generate(&CaParams::default()).unwrap();
        store
            .install_api_pair(&validate_server_pair(&first.cert_pem, &first.key_pem).unwrap())
            .unwrap();
        assert!(
            !dir.path().join(API_ARCHIVE_DIR).exists(),
            "a first import has nothing to archive"
        );

        let second = ca::generate(&CaParams::default()).unwrap();
        store
            .install_api_pair(&validate_server_pair(&second.cert_pem, &second.key_pem).unwrap())
            .unwrap();

        let stamped = fs::read_dir(dir.path().join(API_ARCHIVE_DIR))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            fs::read_to_string(stamped.join(API_KEY_FILE)).unwrap(),
            first.key_pem,
            "the replaced private key is preserved, not deleted"
        );
        assert_eq!(
            all_certificates(
                &fs::read_to_string(stamped.join(API_CERT_FILE)).unwrap(),
                "the certificate"
            )
            .unwrap(),
            all_certificates(&first.cert_pem, "the certificate").unwrap()
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(API_KEY_FILE)).unwrap(),
            second.key_pem,
            "the live pair is the newly imported one"
        );
    }

    #[test]
    fn the_api_pair_source_is_answered_without_reading_the_disk() {
        let (dir, store) = store();
        let generated = ca::generate(&CaParams::default()).unwrap();
        store
            .install_api_pair(
                &validate_server_pair(&generated.cert_pem, &generated.key_pem).unwrap(),
            )
            .unwrap();
        assert_eq!(store.api_pair_source(), ApiPairSource::Imported);

        fs::remove_file(dir.path().join(API_SOURCE_FILE)).unwrap();
        assert_eq!(
            store.api_pair_source(),
            ApiPairSource::Imported,
            "the marker is read once at open, not on every status call"
        );
        assert_eq!(
            CertStore::open(dir.path()).unwrap().api_pair_source(),
            ApiPairSource::SelfSigned,
            "a reopen is what re-reads the marker"
        );
    }

    #[test]
    fn regenerating_a_self_signed_api_pair_clears_the_imported_marker() {
        let (dir, store) = store();
        let generated = ca::generate(&CaParams::default()).unwrap();
        store
            .install_api_pair(
                &validate_server_pair(&generated.cert_pem, &generated.key_pem).unwrap(),
            )
            .unwrap();
        assert!(dir.path().join(API_SOURCE_FILE).exists());

        for file in [API_CERT_FILE, API_KEY_FILE] {
            fs::remove_file(dir.path().join(file)).unwrap();
        }
        crate::api::load_or_generate(dir.path(), "127.0.0.1", None).unwrap();

        assert!(
            !dir.path().join(API_SOURCE_FILE).exists(),
            "a regenerated self-signed pair must not keep claiming to be imported"
        );
        assert_eq!(
            CertStore::open(dir.path()).unwrap().api_pair_source(),
            ApiPairSource::SelfSigned
        );
    }

    #[test]
    fn a_full_authority_archive_refuses_regeneration_and_keeps_the_live_pair() {
        let (dir, store) = store();
        for _ in 0..=MAX_ARCHIVES {
            store.generate_ca(&CaParams::default()).unwrap();
        }
        let summary = store.ca_summary().unwrap();
        let cert = fs::read_to_string(dir.path().join(CA_CERT_FILE)).unwrap();
        let key = fs::read_to_string(dir.path().join(CA_KEY_FILE)).unwrap();

        assert!(matches!(
            store.generate_ca(&CaParams::default()),
            Err(CertError::ArchiveFull {
                archive: CA_ARCHIVE_DIR,
                limit: MAX_ARCHIVES
            })
        ));
        assert_eq!(store.ca_summary(), Some(summary));
        assert_eq!(
            fs::read_to_string(dir.path().join(CA_CERT_FILE)).unwrap(),
            cert
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(CA_KEY_FILE)).unwrap(),
            key
        );
        assert_eq!(
            fs::read_dir(dir.path().join(CA_ARCHIVE_DIR))
                .unwrap()
                .count(),
            MAX_ARCHIVES,
            "the cap is on retired pairs, and no archive is ever deleted"
        );
        assert!(!dir.path().join(CA_CERT_TMP_FILE).exists());
        assert!(!dir.path().join(CA_KEY_TMP_FILE).exists());
    }

    #[test]
    fn a_full_api_archive_refuses_import_and_keeps_the_live_pair() {
        let (dir, store) = store();
        crate::load_or_generate(dir.path(), "127.0.0.1", None).unwrap();
        for _ in 0..MAX_ARCHIVES {
            let generated = ca::generate(&CaParams::default()).unwrap();
            store
                .install_api_pair(
                    &validate_server_pair(&generated.cert_pem, &generated.key_pem).unwrap(),
                )
                .unwrap();
        }
        let key = fs::read_to_string(dir.path().join(API_KEY_FILE)).unwrap();

        let refused = ca::generate(&CaParams::default()).unwrap();
        assert!(matches!(
            store.install_api_pair(
                &validate_server_pair(&refused.cert_pem, &refused.key_pem).unwrap()
            ),
            Err(CertError::ArchiveFull {
                archive: API_ARCHIVE_DIR,
                limit: MAX_ARCHIVES
            })
        ));
        assert_eq!(
            fs::read_to_string(dir.path().join(API_KEY_FILE)).unwrap(),
            key
        );
        assert!(store.api_certified_key().is_ok());
        assert_eq!(store.api_pair_source(), ApiPairSource::Imported);
        assert!(!dir.path().join(API_KEY_TMP_FILE).exists());
    }

    #[test]
    fn a_marker_that_cannot_be_written_fails_before_the_live_pair_changes() {
        let (dir, store) = store();
        crate::load_or_generate(dir.path(), "127.0.0.1", None).unwrap();
        let cert = fs::read_to_string(dir.path().join(API_CERT_FILE)).unwrap();
        let key = fs::read_to_string(dir.path().join(API_KEY_FILE)).unwrap();
        fs::create_dir(dir.path().join(API_SOURCE_FILE)).unwrap();

        let generated = ca::generate(&CaParams::default()).unwrap();
        assert!(matches!(
            store.install_api_pair(
                &validate_server_pair(&generated.cert_pem, &generated.key_pem).unwrap()
            ),
            Err(CertError::Io { .. })
        ));
        assert_eq!(
            fs::read_to_string(dir.path().join(API_CERT_FILE)).unwrap(),
            cert,
            "an import that cannot record itself must not replace the pair"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(API_KEY_FILE)).unwrap(),
            key
        );
        assert_eq!(store.api_pair_source(), ApiPairSource::SelfSigned);
        assert!(!dir.path().join(API_CERT_TMP_FILE).exists());
        assert!(!dir.path().join(API_KEY_TMP_FILE).exists());
    }

    #[test]
    fn an_archive_that_cannot_be_completed_leaves_no_partial_directory() {
        let (dir, store) = store();
        let generated = ca::generate(&CaParams::default()).unwrap();
        fs::write(dir.path().join(API_CERT_FILE), &generated.cert_pem).unwrap();
        fs::create_dir(dir.path().join(API_KEY_FILE)).unwrap();

        let replacement = ca::generate(&CaParams::default()).unwrap();
        assert!(matches!(
            store.install_api_pair(
                &validate_server_pair(&replacement.cert_pem, &replacement.key_pem).unwrap()
            ),
            Err(CertError::Io { .. })
        ));
        assert_eq!(
            fs::read_dir(dir.path().join(API_ARCHIVE_DIR))
                .unwrap()
                .count(),
            0,
            "a half-copied archive must not count toward the cap"
        );
        assert!(!dir.path().join(API_CERT_TMP_FILE).exists());
        assert!(!dir.path().join(API_KEY_TMP_FILE).exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_commit_that_fails_after_the_certificate_landed_restores_the_archived_pair() {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;

        let (dir, store) = store();
        crate::load_or_generate(dir.path(), "127.0.0.1", None).unwrap();
        let cert = fs::read_to_string(dir.path().join(API_CERT_FILE)).unwrap();
        let key = fs::read_to_string(dir.path().join(API_KEY_FILE)).unwrap();

        let pinned = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(dir.path().join(API_KEY_FILE))
            .unwrap();

        let generated = ca::generate(&CaParams::default()).unwrap();
        assert!(matches!(
            store.install_api_pair(
                &validate_server_pair(&generated.cert_pem, &generated.key_pem).unwrap()
            ),
            Err(CertError::Io { .. })
        ));
        drop(pinned);

        assert_eq!(
            fs::read_to_string(dir.path().join(API_CERT_FILE)).unwrap(),
            cert,
            "the certificate the failed commit had already renamed in must be put back"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(API_KEY_FILE)).unwrap(),
            key
        );
        assert!(!dir.path().join(API_SOURCE_FILE).exists());
        assert_eq!(store.api_pair_source(), ApiPairSource::SelfSigned);
        assert!(store.api_certified_key().is_ok());
        assert!(!dir.path().join(API_CERT_TMP_FILE).exists());
        assert!(!dir.path().join(API_KEY_TMP_FILE).exists());
        assert_eq!(
            fs::read_dir(dir.path().join(API_ARCHIVE_DIR))
                .unwrap()
                .count(),
            1,
            "the archive taken before the commit is kept"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_staged_key_is_never_readable_by_others_even_before_it_is_committed() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let paths = PairPaths::api(dir.path());
        stage_pair(&paths, "cert", "key").unwrap();
        let mode = fs::metadata(&paths.key_tmp).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        fs::write(&paths.key_tmp, "stale, world-readable").unwrap();
        fs::set_permissions(&paths.key_tmp, fs::Permissions::from_mode(0o644)).unwrap();
        stage_pair(&paths, "cert", "key").unwrap();
        let mode = fs::metadata(&paths.key_tmp).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "a stale staged key is replaced, not reused"
        );
    }

    #[test]
    fn an_archived_pair_copied_back_is_not_reported_as_imported() {
        let (dir, store) = store();
        crate::load_or_generate(dir.path(), "127.0.0.1", None).unwrap();
        let generated = ca::generate(&CaParams::default()).unwrap();
        store
            .install_api_pair(
                &validate_server_pair(&generated.cert_pem, &generated.key_pem).unwrap(),
            )
            .unwrap();
        assert_eq!(
            CertStore::open(dir.path()).unwrap().api_pair_source(),
            ApiPairSource::Imported
        );

        let archive = fs::read_dir(dir.path().join(API_ARCHIVE_DIR))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        for file in [API_CERT_FILE, API_KEY_FILE] {
            fs::copy(archive.join(file), dir.path().join(file)).unwrap();
        }

        assert!(
            dir.path().join(API_SOURCE_FILE).exists(),
            "the marker survives a copy-back; the fingerprint inside it is what refutes it"
        );
        assert_eq!(
            CertStore::open(dir.path()).unwrap().api_pair_source(),
            ApiPairSource::SelfSigned,
            "a self-signed pair restored from the archive must not claim to be imported"
        );
    }

    #[test]
    fn concurrent_api_imports_are_serialized_and_never_commit_a_mismatched_pair() {
        const THREADS: usize = 8;
        let (dir, store) = store();
        crate::load_or_generate(dir.path(), "127.0.0.1", None).unwrap();
        let pairs = (0..THREADS)
            .map(|_| ca::generate(&CaParams::default()).unwrap())
            .collect::<Vec<_>>();
        let barrier = std::sync::Barrier::new(THREADS);

        std::thread::scope(|scope| {
            for generated in &pairs {
                let (store, barrier) = (&store, &barrier);
                scope.spawn(move || {
                    let pair =
                        validate_server_pair(&generated.cert_pem, &generated.key_pem).unwrap();
                    barrier.wait();
                    store.install_api_pair(&pair).unwrap();
                });
            }
        });

        assert!(
            store.api_certified_key().is_ok(),
            "the live pair must be key-matched after overlapping imports"
        );
        let live_key = fs::read_to_string(dir.path().join(API_KEY_FILE)).unwrap();
        let live_cert = fs::read_to_string(dir.path().join(API_CERT_FILE)).unwrap();
        let winner = pairs
            .iter()
            .find(|generated| generated.key_pem == live_key)
            .expect("the live key is one of the imported keys");
        assert_eq!(
            all_certificates(&live_cert, "the certificate").unwrap(),
            all_certificates(&winner.cert_pem, "the certificate").unwrap(),
            "the live certificate belongs to the live key"
        );
        assert_eq!(
            fs::read_dir(dir.path().join(API_ARCHIVE_DIR))
                .unwrap()
                .count(),
            THREADS,
            "every import archived exactly the pair it replaced"
        );
        assert!(!dir.path().join(API_CERT_TMP_FILE).exists());
        assert!(!dir.path().join(API_KEY_TMP_FILE).exists());
        assert_eq!(store.api_pair_source(), ApiPairSource::Imported);
    }

    #[test]
    fn an_imported_authority_never_writes_or_exports_private_material() {
        let (dir, store) = store();
        let generated = ca::generate(&CaParams::default()).unwrap();
        let blob = format!("{}{}", generated.cert_pem, generated.key_pem);

        let pair = validate_ca_pair(&blob, &generated.key_pem).unwrap();
        store.install_ca_pair(&pair).unwrap();

        let exported = store.ca_public_pem().unwrap();
        assert!(!exported.contains("PRIVATE"));
        assert_eq!(exported.matches("BEGIN CERTIFICATE").count(), 1);

        let on_disk = fs::read_to_string(dir.path().join(CA_CERT_FILE)).unwrap();
        assert!(
            !on_disk.contains("PRIVATE"),
            "the certificate file must never receive key material pasted into the cert field"
        );
        assert!(!String::from_utf8_lossy(&store.ca_public_der().unwrap()).contains("PRIVATE"));
    }

    #[test]
    fn an_api_pair_carrying_pasted_key_material_is_written_certificate_only() {
        let (dir, store) = store();
        let generated = ca::generate(&CaParams::default()).unwrap();
        let blob = format!("{}{}", generated.cert_pem, generated.key_pem);

        let pair = validate_server_pair(&blob, &generated.key_pem).unwrap();
        store.install_api_pair(&pair).unwrap();

        let on_disk = fs::read_to_string(dir.path().join(API_CERT_FILE)).unwrap();
        assert!(!on_disk.contains("PRIVATE"));
    }

    #[test]
    fn an_authority_on_disk_that_is_not_a_ca_is_refused_at_open() {
        let (dir, _store) = store();
        crate::install_crypto_provider();
        let mut params = rcgen::CertificateParams::new(vec!["leaf.example".to_string()]).unwrap();
        params.is_ca = rcgen::IsCa::ExplicitNoCa;
        let key_pair = rcgen::KeyPair::generate().unwrap();
        let leaf = params.self_signed(&key_pair).unwrap();

        fs::write(dir.path().join(CA_CERT_FILE), leaf.pem()).unwrap();
        fs::write(dir.path().join(CA_KEY_FILE), key_pair.serialize_pem()).unwrap();

        assert!(matches!(
            CertStore::open(dir.path()),
            Err(CertError::NotACa)
        ));
    }

    #[test]
    fn an_authority_whose_key_does_not_match_is_refused_at_open() {
        let (dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();
        let foreign = ca::generate(&CaParams::default()).unwrap();
        fs::write(dir.path().join(CA_KEY_FILE), &foreign.key_pem).unwrap();

        assert!(matches!(
            CertStore::open(dir.path()),
            Err(CertError::KeyMismatch)
        ));
    }

    #[test]
    fn an_interrupted_regeneration_is_refused_loudly_and_keeps_the_staged_key() {
        let (dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();
        let old_key = fs::read_to_string(dir.path().join(CA_KEY_FILE)).unwrap();
        let replacement = ca::generate(&CaParams::default()).unwrap();

        fs::write(dir.path().join(CA_CERT_FILE), &replacement.cert_pem).unwrap();
        fs::write(dir.path().join(CA_KEY_TMP_FILE), &replacement.key_pem).unwrap();

        assert!(matches!(
            CertStore::open(dir.path()),
            Err(CertError::KeyMismatch)
        ));
        assert_eq!(
            fs::read_to_string(dir.path().join(CA_KEY_TMP_FILE)).unwrap(),
            replacement.key_pem,
            "the staged key must survive a refused open so the pair can be completed"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(CA_KEY_FILE)).unwrap(),
            old_key
        );
    }

    #[test]
    fn the_api_certified_key_refuses_a_pair_whose_key_does_not_match() {
        let (dir, store) = store();
        crate::load_or_generate(dir.path(), "127.0.0.1", None).unwrap();
        let foreign = ca::generate(&CaParams::default()).unwrap();
        fs::write(dir.path().join(API_KEY_FILE), &foreign.key_pem).unwrap();

        assert!(matches!(
            store.api_certified_key(),
            Err(CertError::Config { .. })
        ));
        assert!(matches!(
            crate::load_or_generate(dir.path(), "127.0.0.1", None),
            Err(CertError::Config { .. })
        ));
    }

    #[test]
    fn repeated_regenerations_within_one_second_each_get_their_own_archive() {
        let (dir, store) = store();
        let mut keys = Vec::new();
        for _ in 0..4 {
            store.generate_ca(&CaParams::default()).unwrap();
            keys.push(fs::read_to_string(dir.path().join(CA_KEY_FILE)).unwrap());
        }

        let archive = dir.path().join(CA_ARCHIVE_DIR);
        let mut archived = fs::read_dir(&archive)
            .unwrap()
            .map(|entry| fs::read_to_string(entry.unwrap().path().join(CA_KEY_FILE)).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(archived.len(), 3, "one archive per replaced authority");

        archived.sort();
        archived.dedup();
        assert_eq!(
            archived.len(),
            3,
            "no archived private key may be overwritten by a later regeneration"
        );
        for key in &keys[..3] {
            assert!(archived.contains(key), "every replaced key is preserved");
        }
    }

    #[test]
    fn a_failed_regeneration_leaves_the_live_authority_untouched() {
        let (dir, store) = store();
        let first = store.generate_ca(&CaParams::default()).unwrap();
        let cert = fs::read_to_string(dir.path().join(CA_CERT_FILE)).unwrap();
        let key = fs::read_to_string(dir.path().join(CA_KEY_FILE)).unwrap();

        fs::create_dir(dir.path().join(CA_CERT_TMP_FILE)).unwrap();
        assert!(store.generate_ca(&CaParams::default()).is_err());

        assert_eq!(store.ca_summary(), Some(first.summary.clone()));
        assert_eq!(
            fs::read_to_string(dir.path().join(CA_CERT_FILE)).unwrap(),
            cert
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(CA_KEY_FILE)).unwrap(),
            key
        );
        assert!(
            !dir.path().join(CA_ARCHIVE_DIR).exists(),
            "nothing is archived until the replacement is staged"
        );
        assert!(!dir.path().join(CA_KEY_TMP_FILE).exists());

        fs::remove_dir(dir.path().join(CA_CERT_TMP_FILE)).unwrap();
        assert_eq!(
            CertStore::open(dir.path()).unwrap().ca_summary(),
            Some(first.summary)
        );
    }

    #[test]
    fn a_cache_read_never_mints_and_only_a_prewarm_does() {
        let (_dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();

        assert!(store.cached_leaf("a.example").is_none());
        let cold = store.leaf_cache_stats();
        assert_eq!(cold.minted_total, 0, "a cache read must never mint");
        assert_eq!((cold.unwarmed_misses, cold.size), (1, 0));

        let warmed = store.prewarm("a.example").unwrap();
        let served = store.cached_leaf("a.example").unwrap();
        assert!(Arc::ptr_eq(&warmed, &served));

        let warm = store.leaf_cache_stats();
        assert_eq!((warm.minted_total, warm.hits), (1, 1));
        assert_eq!((warm.unwarmed_misses, warm.inflight), (1, 0));
    }

    #[test]
    fn a_failed_mint_releases_the_host_instead_of_stranding_waiters() {
        let (_dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();

        let rejected = "ü.example";
        assert!(store.prewarm(rejected).is_err());
        let after = store.leaf_cache_stats();
        assert_eq!(
            after.inflight, 0,
            "a failed mint must release its in-flight slot"
        );
        assert_eq!((after.size, after.minted_total), (0, 0));

        assert!(
            store.prewarm(rejected).is_err(),
            "a retry must fail again rather than block on a stale in-flight marker"
        );
        assert_eq!(store.leaf_cache_stats().inflight, 0);
    }

    #[test]
    fn a_cache_read_without_an_authority_mints_nothing_and_does_not_error() {
        let (_dir, store) = store();
        assert!(store.cached_leaf("a.example").is_none());
        assert!(matches!(store.prewarm("a.example"), Err(CertError::NoCa)));
        assert_eq!(store.leaf_cache_stats().minted_total, 0);
    }

    #[test]
    fn a_prewarmed_entry_is_dropped_when_the_authority_is_replaced() {
        let (_dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();
        store.prewarm("a.example").unwrap();
        assert!(store.cached_leaf("a.example").is_some());

        store.generate_ca(&CaParams::default()).unwrap();
        assert!(
            store.cached_leaf("a.example").is_none(),
            "a leaf signed by the archived authority must never be served again"
        );
        assert_eq!(store.leaf_cache_stats().size, 0);
    }

    #[test]
    fn the_cache_cap_holds_across_many_prewarms() {
        let (_dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();
        for index in 0..(LEAF_CACHE_CAPACITY + 88) {
            store.prewarm(&format!("host{index}.example")).unwrap();
        }

        let stats = store.leaf_cache_stats();
        assert_eq!(stats.size, LEAF_CACHE_CAPACITY);
        assert_eq!(stats.capacity, LEAF_CACHE_CAPACITY);
        assert_eq!(stats.evictions, 88);
        assert_eq!(stats.inflight, 0);
    }

    #[test]
    fn concurrent_prewarm_and_cache_reads_stay_consistent() {
        const THREADS: usize = 12;
        let (_dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();
        let barrier = std::sync::Barrier::new(THREADS * 2);

        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                let (store, barrier) = (&store, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    store.prewarm("shared.example").unwrap();
                });
                scope.spawn(move || {
                    barrier.wait();
                    let _ = store.cached_leaf("shared.example");
                });
            }
        });

        let stats = store.leaf_cache_stats();
        assert_eq!(stats.minted_total, 1, "single-flight across the store");
        assert_eq!((stats.size, stats.inflight), (1, 0));
        assert_eq!(stats.hits + stats.unwarmed_misses, THREADS as u64);
    }

    #[test]
    fn a_host_is_matched_case_insensitively_and_bounded_in_length() {
        let (_dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();

        let lower = store.prewarm("a.example").unwrap();
        let mixed = store.prewarm("A.Example").unwrap();
        assert!(
            Arc::ptr_eq(&lower, &mixed),
            "SNI is case-insensitive; one host must not occupy two cache entries"
        );
        assert!(Arc::ptr_eq(
            &lower,
            &store.cached_leaf("A.EXAMPLE").unwrap()
        ));
        assert_eq!(store.leaf_cache_stats().size, 1);
        assert_eq!(store.leaf_cache_stats().minted_total, 1);

        assert!(store.cached_leaf("").is_none());
        assert!(store.cached_leaf(&"a".repeat(MAX_HOST_LEN + 1)).is_none());

        assert!(matches!(store.prewarm(""), Err(CertError::InvalidHost)));
        assert!(matches!(
            store.prewarm(&"a".repeat(MAX_HOST_LEN + 1)),
            Err(CertError::InvalidHost)
        ));
        assert!(store.prewarm(&"a".repeat(MAX_HOST_LEN)).is_ok());
    }

    #[test]
    fn an_expired_authority_mints_nothing_and_a_short_lived_one_clamps_its_leaves() {
        let (_expired_dir, expired) = store();
        expired
            .generate_ca(&CaParams {
                common_name: "Expired Authority".to_string(),
                validity_days: 0,
            })
            .unwrap();
        assert!(matches!(
            expired.prewarm("a.example"),
            Err(CertError::Expired { .. })
        ));
        assert_eq!(expired.leaf_cache_stats().minted_total, 0);

        let (_dir, store) = store();
        let summary = store
            .generate_ca(&CaParams {
                common_name: "Short Lived Authority".to_string(),
                validity_days: 2,
            })
            .unwrap()
            .summary;
        let leaf = store.prewarm("a.example").unwrap();
        let (_, parsed) = x509_parser::parse_x509_certificate(&leaf.cert[0]).unwrap();
        assert_eq!(
            parsed.validity().not_after.timestamp(),
            summary.not_after,
            "a leaf must never outlive the authority that signed it"
        );
    }

    #[test]
    fn the_generated_authority_may_not_issue_intermediate_authorities() {
        let (_dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();

        let der = store.ca_public_der().unwrap();
        let (_, parsed) = x509_parser::parse_x509_certificate(&der).unwrap();
        let constraints = parsed.basic_constraints().unwrap().unwrap();
        assert!(constraints.value.ca);
        assert_eq!(constraints.value.path_len_constraint, Some(0));
    }

    #[test]
    fn the_api_certified_key_is_the_pair_on_disk() {
        let (dir, store) = store();
        assert!(matches!(
            store.api_certified_key(),
            Err(CertError::Empty { .. })
        ));

        crate::load_or_generate(dir.path(), "127.0.0.1", None).unwrap();
        let certified = store.api_certified_key().unwrap();

        let on_disk = all_certificates(
            &fs::read_to_string(dir.path().join(API_CERT_FILE)).unwrap(),
            "the certificate",
        )
        .unwrap();
        assert_eq!(certified.cert, on_disk);
    }

    #[test]
    fn the_store_debug_output_carries_no_key_material() {
        let (_dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();
        let rendered = format!("{store:?}");
        assert!(!rendered.contains("PRIVATE"));
    }

    #[cfg(unix)]
    #[test]
    fn the_authority_key_is_written_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, store) = store();
        store.generate_ca(&CaParams::default()).unwrap();
        let mode = fs::metadata(dir.path().join(CA_KEY_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

use std::collections::HashMap;
use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use arc_swap::ArcSwap;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};

use crate::session::{self, SessionSecret, SESSION_LIFETIME};

pub const HASH_FILE: &str = "auth-hash";
const HASH_TMP_FILE: &str = "auth-hash.tmp";

const ARGON2_MEMORY_KIB: u32 = 19_456;
const ARGON2_ITERATIONS: u32 = 2;
const ARGON2_PARALLELISM: u32 = 1;

const ARGON2_PERMITS: usize = 2;

const GENERATED_PASSWORD_BYTES: usize = 16;
const BASE32_ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

pub const MIN_PASSWORD_CHARS: usize = 12;

const PRODUCTION_LIMITS: RateLimits = RateLimits {
    per_address: 5,
    global: 30,
    window: Duration::from_secs(60),
    max_tracked: 128,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimits {
    pub per_address: u32,
    pub global: u32,
    pub window: Duration,
    pub max_tracked: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateDecision {
    Allowed,
    Limited { retry_after: u64 },
}

struct Bucket {
    count: u32,
    window_start: Instant,
}

impl Bucket {
    fn new(now: Instant) -> Self {
        Self {
            count: 0,
            window_start: now,
        }
    }

    fn roll(&mut self, now: Instant, window: Duration) {
        if now.duration_since(self.window_start) >= window {
            self.count = 0;
            self.window_start = now;
        }
    }

    fn expired(&self, now: Instant, window: Duration) -> bool {
        now.duration_since(self.window_start) >= window
    }

    fn retry_after(&self, now: Instant, window: Duration) -> u64 {
        let remaining = window.saturating_sub(now.duration_since(self.window_start));
        remaining.as_secs().max(1)
    }
}

struct RateLimiter {
    limits: RateLimits,
    per_address: HashMap<IpAddr, Bucket>,
    global: Bucket,
}

impl RateLimiter {
    fn new(limits: RateLimits, now: Instant) -> Self {
        Self {
            limits,
            per_address: HashMap::new(),
            global: Bucket::new(now),
        }
    }

    fn check(&mut self, address: IpAddr, now: Instant) -> RateDecision {
        let window = self.limits.window;

        if let Some(bucket) = self.per_address.get_mut(&address) {
            bucket.roll(now, window);
            bucket.count += 1;
            if bucket.count > self.limits.per_address {
                return RateDecision::Limited {
                    retry_after: bucket.retry_after(now, window),
                };
            }
        } else {
            if self.per_address.len() >= self.limits.max_tracked {
                self.per_address
                    .retain(|_, bucket| !bucket.expired(now, window));
            }
            if self.per_address.len() >= self.limits.max_tracked {
                self.global.roll(now, window);
                self.global.count += 1;
                return RateDecision::Limited {
                    retry_after: self.global.retry_after(now, window),
                };
            }
            let mut bucket = Bucket::new(now);
            bucket.count = 1;
            self.per_address.insert(address, bucket);
        }

        self.global.roll(now, window);
        self.global.count += 1;
        if self.global.count > self.limits.global {
            return RateDecision::Limited {
                retry_after: self.global.retry_after(now, window),
            };
        }

        RateDecision::Allowed
    }
}

pub struct Argon2Permit(#[allow(dead_code)] OwnedSemaphorePermit);

pub struct AuthState {
    hash_path: PathBuf,
    hash_tmp: PathBuf,
    data_dir: PathBuf,
    hash: ArcSwap<String>,
    secret: RwLock<SessionSecret>,
    argon2: Arc<Semaphore>,
    limiter: Mutex<RateLimiter>,
}

impl AuthState {
    pub fn load_or_create(
        config_dir: &Path,
        data_dir: &Path,
    ) -> io::Result<(Self, Option<String>)> {
        Self::build(config_dir, data_dir, PRODUCTION_LIMITS)
    }

    #[cfg(feature = "test-harness")]
    pub fn load_or_create_with_limits(
        config_dir: &Path,
        data_dir: &Path,
        limits: RateLimits,
    ) -> io::Result<(Self, Option<String>)> {
        Self::build(config_dir, data_dir, limits)
    }

    #[cfg(feature = "test-harness")]
    pub fn for_tests(
        config_dir: &Path,
        data_dir: &Path,
        password: &str,
        limits: RateLimits,
    ) -> io::Result<Self> {
        let hash = hash_password(password).map_err(io::Error::other)?;
        stage_write(
            &config_dir.join(HASH_FILE),
            &config_dir.join(HASH_TMP_FILE),
            &hash,
        )?;
        let _ = fs::remove_file(data_dir.join(session::SECRET_FILE));
        let (state, _) = Self::build(config_dir, data_dir, limits)?;
        Ok(state)
    }

    #[cfg(feature = "test-harness")]
    pub const fn relaxed_limits() -> RateLimits {
        RateLimits {
            per_address: u32::MAX,
            global: u32::MAX,
            window: Duration::from_secs(60),
            max_tracked: 128,
        }
    }

    #[cfg(feature = "test-harness")]
    pub const fn production_limits() -> RateLimits {
        PRODUCTION_LIMITS
    }

    #[cfg(feature = "test-harness")]
    pub async fn mint_for_tests(&self, version: u8, expiry: SystemTime) -> String {
        self.secret.read().await.mint_with_version(version, expiry)
    }

    #[cfg(feature = "test-harness")]
    pub async fn hash_for_tests(permit: Argon2Permit, password: &str) -> Result<String, String> {
        hash_password_off_runtime(permit, password.to_string()).await
    }

    fn build(
        config_dir: &Path,
        data_dir: &Path,
        limits: RateLimits,
    ) -> io::Result<(Self, Option<String>)> {
        let hash_path = config_dir.join(HASH_FILE);
        let hash_tmp = config_dir.join(HASH_TMP_FILE);
        discard_stray_tmp(&hash_tmp);

        let stored_hash = fs::read_to_string(&hash_path)
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
        let reset = stored_hash.is_none();

        let stored_secret = session::load_secret(data_dir);
        let secret = match stored_secret {
            Some(secret) if !reset => secret,
            other => {
                if other.is_none() && !reset {
                    tracing::warn!(
                        path = %data_dir.join(session::SECRET_FILE).display(),
                        "no session secret in /data — generating one; every existing \
                         session is now invalid and the password is unchanged"
                    );
                }
                session::write_fresh_secret(data_dir)?
            }
        };

        let (hash, generated) = match stored_hash {
            Some(hash) => (hash, None),
            None => {
                let password = generate_password();
                let hash = hash_password(&password).map_err(io::Error::other)?;
                stage_write(&hash_path, &hash_tmp, &hash)?;
                (hash, Some(password))
            }
        };

        Ok((
            Self {
                hash_path,
                hash_tmp,
                data_dir: data_dir.to_path_buf(),
                hash: ArcSwap::from_pointee(hash),
                secret: RwLock::new(secret),
                argon2: Arc::new(Semaphore::new(ARGON2_PERMITS)),
                limiter: Mutex::new(RateLimiter::new(limits, Instant::now())),
            },
            generated,
        ))
    }

    pub fn check_rate(&self, address: IpAddr) -> RateDecision {
        let mut limiter = self
            .limiter
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        limiter.check(address, Instant::now())
    }

    pub fn try_argon2_permit(&self) -> Option<Argon2Permit> {
        Arc::clone(&self.argon2)
            .try_acquire_owned()
            .ok()
            .map(Argon2Permit)
    }

    pub async fn verify_session(&self, token: &str) -> bool {
        self.secret.read().await.verify(token, SystemTime::now())
    }

    pub async fn verify_and_mint(
        &self,
        permit: Argon2Permit,
        candidate: String,
    ) -> Result<Option<String>, String> {
        let guard = self.secret.read().await;
        let hash = self.hash.load_full();
        let (matched, permit) = verify_password(permit, hash, candidate).await?;
        drop(permit);
        if !matched {
            return Ok(None);
        }
        Ok(Some(guard.mint(SystemTime::now() + SESSION_LIFETIME)))
    }

    pub async fn verify_only(
        &self,
        permit: Argon2Permit,
        candidate: String,
    ) -> Result<(bool, Argon2Permit), String> {
        verify_password(permit, self.hash.load_full(), candidate).await
    }

    pub async fn rotate_secret(&self) -> io::Result<()> {
        let mut guard = self.secret.write().await;
        *guard = session::write_fresh_secret(&self.data_dir)?;
        Ok(())
    }

    pub async fn replace_password(&self, new_hash: String) -> io::Result<()> {
        let mut guard = self.secret.write().await;
        *guard = session::write_fresh_secret(&self.data_dir)?;
        stage_write(&self.hash_path, &self.hash_tmp, &new_hash)?;
        self.hash.store(Arc::new(new_hash));
        Ok(())
    }
}

pub async fn hash_password_off_runtime(
    permit: Argon2Permit,
    password: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        hash_password(&password)
    })
    .await
    .map_err(|err| err.to_string())?
}

async fn verify_password(
    permit: Argon2Permit,
    hash: Arc<String>,
    candidate: String,
) -> Result<(bool, Argon2Permit), String> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(hash.as_str()).map_err(|err| err.to_string())?;
        let matched = argon2()
            .verify_password(candidate.as_bytes(), &parsed)
            .is_ok();
        Ok((matched, permit))
    })
    .await
    .map_err(|err| err.to_string())?
}

fn argon2() -> Argon2<'static> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        None,
    )
    .expect("the compiled-in Argon2id parameters are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

fn hash_password(password: &str) -> Result<String, String> {
    let mut salt_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|err| err.to_string())?;
    argon2()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| err.to_string())
}

fn generate_password() -> String {
    let mut bytes = [0u8; GENERATED_PASSWORD_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    let mut out = String::with_capacity(GENERATED_PASSWORD_BYTES * 8 / 5 + 1);
    let (mut accumulator, mut bits) = (0u16, 0u32);
    for byte in bytes {
        accumulator = (accumulator << 8) | u16::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let index = ((accumulator >> bits) & 0x1f) as usize;
            out.push(BASE32_ALPHABET[index] as char);
        }
    }
    if bits > 0 {
        let index = ((accumulator << (5 - bits)) & 0x1f) as usize;
        out.push(BASE32_ALPHABET[index] as char);
    }
    out
}

pub fn discard_stray_tmp(tmp: &Path) {
    if tmp.exists() {
        tracing::warn!(
            path = %tmp.display(),
            "discarding an interrupted auth write — a partial file is not a credential"
        );
        let _ = fs::remove_file(tmp);
    }
}

pub fn stage_write(path: &Path, tmp: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    if let Err(error) = fs::write(tmp, contents).and_then(|()| restrict_permissions(tmp)) {
        let _ = fs::remove_file(tmp);
        return Err(error);
    }
    if let Err(error) = fs::rename(tmp, path) {
        let _ = fs::remove_file(tmp);
        return Err(error);
    }
    restrict_permissions(path)
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    fn address(last: u32) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + last))
    }

    fn limits() -> RateLimits {
        PRODUCTION_LIMITS
    }

    #[test]
    fn a_generated_password_is_twenty_six_base32_characters() {
        let password = generate_password();
        assert_eq!(password.len(), 26);
        assert!(password.bytes().all(|byte| BASE32_ALPHABET.contains(&byte)));
        assert_ne!(password, generate_password());
    }

    #[test]
    fn the_sixth_attempt_from_one_address_inside_the_window_is_limited() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new(limits(), now);
        let client = address(1);
        for attempt in 1..=5 {
            assert_eq!(
                limiter.check(client, now),
                RateDecision::Allowed,
                "attempt {attempt}"
            );
        }
        let RateDecision::Limited { retry_after } = limiter.check(client, now) else {
            panic!("the sixth attempt must be rejected");
        };
        assert!((1..=60).contains(&retry_after));
    }

    #[test]
    fn the_window_rolls_and_the_address_is_allowed_again() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new(limits(), now);
        let client = address(2);
        for _ in 0..6 {
            limiter.check(client, now);
        }
        let later = now + Duration::from_secs(61);
        assert_eq!(limiter.check(client, later), RateDecision::Allowed);
    }

    #[test]
    fn the_global_cap_rejects_a_distributed_burst() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new(limits(), now);
        for index in 0..30 {
            assert_eq!(limiter.check(address(index), now), RateDecision::Allowed);
        }
        assert!(matches!(
            limiter.check(address(999), now),
            RateDecision::Limited { .. }
        ));
    }

    #[test]
    fn the_tracked_map_stays_bounded_under_ten_thousand_addresses() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new(limits(), now);
        for index in 0..10_000 {
            limiter.check(address(index), now);
            assert!(
                limiter.per_address.len() <= limits().max_tracked,
                "map grew to {}",
                limiter.per_address.len()
            );
        }
        assert!(limiter.per_address.len() <= 128);
    }

    #[test]
    fn a_full_map_falls_back_to_the_global_bucket_and_fails_closed() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new(
            RateLimits {
                per_address: u32::MAX,
                global: u32::MAX,
                window: Duration::from_secs(60),
                max_tracked: 4,
            },
            now,
        );
        for index in 0..4 {
            assert_eq!(limiter.check(address(index), now), RateDecision::Allowed);
        }
        assert!(matches!(
            limiter.check(IpAddr::V6(Ipv6Addr::LOCALHOST), now),
            RateDecision::Limited { .. }
        ));
        assert_eq!(limiter.per_address.len(), 4);
    }

    #[test]
    fn expired_entries_are_evicted_before_the_fallback() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new(
            RateLimits {
                per_address: u32::MAX,
                global: u32::MAX,
                window: Duration::from_secs(60),
                max_tracked: 4,
            },
            now,
        );
        for index in 0..4 {
            limiter.check(address(index), now);
        }
        let later = now + Duration::from_secs(61);
        assert_eq!(
            limiter.check(IpAddr::V6(Ipv6Addr::LOCALHOST), later),
            RateDecision::Allowed
        );
        assert_eq!(limiter.per_address.len(), 1);
    }

    #[tokio::test]
    async fn first_boot_generates_persists_the_hash_only_and_reports_once() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let (state, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        let password = generated.expect("first boot reports the generated password once");
        assert_eq!(password.len(), 26);

        let stored = fs::read_to_string(config.path().join(HASH_FILE)).unwrap();
        assert!(
            stored.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "{stored}"
        );
        assert!(!stored.contains(&password));

        let permit = state.try_argon2_permit().unwrap();
        assert!(state
            .verify_and_mint(permit, password.clone())
            .await
            .unwrap()
            .is_some());
        let permit = state.try_argon2_permit().unwrap();
        assert!(state
            .verify_and_mint(permit, "wrong".to_string())
            .await
            .unwrap()
            .is_none());

        let (reloaded, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        assert!(generated.is_none(), "an existing hash is not re-announced");
        let permit = reloaded.try_argon2_permit().unwrap();
        assert!(reloaded
            .verify_and_mint(permit, password)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn both_files_present_means_nothing_is_written_or_rotated() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (state, _) = AuthState::load_or_create(config.path(), data.path()).unwrap();

        let hash_before = fs::read_to_string(config.path().join(HASH_FILE)).unwrap();
        let secret_before = fs::read_to_string(data.path().join(session::SECRET_FILE)).unwrap();
        let token = state
            .secret
            .read()
            .await
            .mint(SystemTime::now() + SESSION_LIFETIME);

        let (reloaded, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        assert!(generated.is_none());
        assert_eq!(
            fs::read_to_string(config.path().join(HASH_FILE)).unwrap(),
            hash_before
        );
        assert_eq!(
            fs::read_to_string(data.path().join(session::SECRET_FILE)).unwrap(),
            secret_before
        );
        assert!(reloaded.verify_session(&token).await);
    }

    #[tokio::test]
    async fn an_out_of_band_reset_regenerates_both_and_kills_every_session() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (state, _) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        let token = state
            .secret
            .read()
            .await
            .mint(SystemTime::now() + SESSION_LIFETIME);
        let secret_before = fs::read_to_string(data.path().join(session::SECRET_FILE)).unwrap();

        fs::remove_file(config.path().join(HASH_FILE)).unwrap();
        let (reset, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        assert!(generated.is_some(), "a reset regenerates the password");
        assert!(!reset.verify_session(&token).await);
        assert_ne!(
            fs::read_to_string(data.path().join(session::SECRET_FILE)).unwrap(),
            secret_before
        );
    }

    #[tokio::test]
    async fn a_missing_secret_regenerates_the_secret_alone_and_keeps_the_password() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (state, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        let password = generated.unwrap();
        let token = state
            .secret
            .read()
            .await
            .mint(SystemTime::now() + SESSION_LIFETIME);
        let hash_before = fs::read_to_string(config.path().join(HASH_FILE)).unwrap();

        fs::remove_file(data.path().join(session::SECRET_FILE)).unwrap();
        let (reloaded, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();

        assert!(generated.is_none(), "no plaintext is returned");
        assert_eq!(
            fs::read_to_string(config.path().join(HASH_FILE)).unwrap(),
            hash_before,
            "the hash is byte-identical"
        );
        assert!(!reloaded.verify_session(&token).await);
        let permit = reloaded.try_argon2_permit().unwrap();
        assert!(reloaded
            .verify_and_mint(permit, password)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn an_empty_hash_file_counts_as_absent_and_takes_the_reset_path() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        AuthState::load_or_create(config.path(), data.path()).unwrap();
        fs::write(config.path().join(HASH_FILE), "   \n").unwrap();

        let (_, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        assert!(
            generated.is_some(),
            "an empty hash is a reset, not a lockout"
        );
    }

    #[tokio::test]
    async fn a_stray_hash_tmp_is_discarded_and_boot_succeeds() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (_, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        let password = generated.unwrap();
        fs::write(config.path().join(HASH_TMP_FILE), "$argon2id$garbage").unwrap();

        let (state, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        assert!(generated.is_none());
        assert!(!config.path().join(HASH_TMP_FILE).exists());
        let permit = state.try_argon2_permit().unwrap();
        assert!(state
            .verify_and_mint(permit, password)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn a_tmp_beside_an_absent_hash_is_never_adopted() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        fs::write(config.path().join(HASH_TMP_FILE), "$argon2id$garbage").unwrap();

        let (_, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        assert!(
            generated.is_some(),
            "an interrupted write is not a credential"
        );
        assert!(!config.path().join(HASH_TMP_FILE).exists());
    }

    #[tokio::test]
    async fn a_crash_between_rotation_and_the_hash_write_leaves_the_old_password() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (state, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        let password = generated.unwrap();
        let token = state
            .secret
            .read()
            .await
            .mint(SystemTime::now() + SESSION_LIFETIME);
        let hash_before = fs::read_to_string(config.path().join(HASH_FILE)).unwrap();

        state.rotate_secret().await.unwrap();

        let (recovered, _) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        assert_eq!(
            fs::read_to_string(config.path().join(HASH_FILE)).unwrap(),
            hash_before,
            "the old password survives"
        );
        assert!(
            !recovered.verify_session(&token).await,
            "no session survives"
        );
        let permit = recovered.try_argon2_permit().unwrap();
        assert!(recovered
            .verify_and_mint(permit, password)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn replacing_the_password_rotates_the_secret_and_persists_both() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (state, _) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        let token = state
            .secret
            .read()
            .await
            .mint(SystemTime::now() + SESSION_LIFETIME);

        let permit = state.try_argon2_permit().unwrap();
        let new_hash = hash_password_off_runtime(permit, "a-long-new-password".to_string())
            .await
            .unwrap();
        state.replace_password(new_hash).await.unwrap();

        assert!(!state.verify_session(&token).await);
        let permit = state.try_argon2_permit().unwrap();
        assert!(state
            .verify_and_mint(permit, "a-long-new-password".to_string())
            .await
            .unwrap()
            .is_some());

        let (reloaded, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        assert!(generated.is_none());
        let permit = reloaded.try_argon2_permit().unwrap();
        assert!(reloaded
            .verify_and_mint(permit, "a-long-new-password".to_string())
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn no_login_racing_a_rotation_yields_a_surviving_session() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (state, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        let password = generated.unwrap();
        let state = Arc::new(state);

        let login = {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                let permit = state.try_argon2_permit().unwrap();
                state.verify_and_mint(permit, password).await.unwrap()
            })
        };
        tokio::time::sleep(Duration::from_millis(5)).await;
        state.rotate_secret().await.unwrap();

        let token = login.await.unwrap().expect("the password is correct");
        assert!(
            !state.verify_session(&token).await,
            "a session minted under the pre-rotation secret must not survive it"
        );
    }

    #[tokio::test]
    async fn the_semaphore_bounds_concurrent_verifications() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (state, _) = AuthState::load_or_create(config.path(), data.path()).unwrap();

        let held: Vec<_> = (0..ARGON2_PERMITS)
            .map(|_| state.try_argon2_permit().expect("a permit is available"))
            .collect();
        assert!(
            state.try_argon2_permit().is_none(),
            "saturation must be reported, not queued"
        );
        drop(held);
        assert!(state.try_argon2_permit().is_some());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_abandoned_verification_keeps_its_permit_until_the_argon2_work_ends() {
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (state, generated) = AuthState::load_or_create(config.path(), data.path()).unwrap();
        let state = Arc::new(state);
        let password = generated.unwrap();

        let mut abandoned = Vec::new();
        for _ in 0..ARGON2_PERMITS {
            let state = Arc::clone(&state);
            let password = password.clone();
            abandoned.push(tokio::spawn(async move {
                let permit = state.try_argon2_permit().expect("a permit is available");
                let _ = state.verify_and_mint(permit, password).await;
            }));
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            state.try_argon2_permit().is_none(),
            "both permits are taken while the two verifications are in flight"
        );

        for handle in &abandoned {
            handle.abort();
        }
        for handle in abandoned {
            let _ = handle.await;
        }

        assert!(
            state.try_argon2_permit().is_none(),
            "dropping the request future must not free a permit while its Argon2 \
             arena is still live: the permit belongs to the blocking task"
        );

        let mut freed = false;
        for _ in 0..600 {
            if state.try_argon2_permit().is_some() {
                freed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            freed,
            "the permit is released once the blocking work returns"
        );
    }
}

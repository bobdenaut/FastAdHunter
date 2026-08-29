use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aws_lc_rs::hmac;
use rand::RngCore;

pub const COOKIE_NAME: &str = "__Host-fah_session";

pub const SECRET_FILE: &str = "session-secret";
const SECRET_TMP_FILE: &str = "session-secret.tmp";
const SECRET_BYTES: usize = 32;

const TOKEN_VERSION: u8 = 1;
const NONCE_BYTES: usize = 16;
const PAYLOAD_BYTES: usize = 1 + 8 + NONCE_BYTES;
const MAC_BYTES: usize = 32;
const TOKEN_CHARS: usize = (PAYLOAD_BYTES + MAC_BYTES) * 2;

pub const SESSION_LIFETIME: Duration = Duration::from_secs(7 * 24 * 3600);

pub struct SessionSecret {
    key: hmac::Key,
}

impl fmt::Debug for SessionSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionSecret(redacted)")
    }
}

impl fmt::Display for SessionSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

impl SessionSecret {
    fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, bytes),
        }
    }

    #[cfg(feature = "test-harness")]
    pub fn mint_with_version(&self, version: u8, expiry: SystemTime) -> String {
        self.mint_raw(version, expiry)
    }

    pub fn mint(&self, expiry: SystemTime) -> String {
        self.mint_raw(TOKEN_VERSION, expiry)
    }

    fn mint_raw(&self, version: u8, expiry: SystemTime) -> String {
        let seconds = expiry
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut payload = [0u8; PAYLOAD_BYTES];
        payload[0] = version;
        payload[1..9].copy_from_slice(&seconds.to_be_bytes());
        rand::rng().fill_bytes(&mut payload[9..]);

        let mac = hmac::sign(&self.key, &payload);
        let mut token = String::with_capacity(TOKEN_CHARS);
        write_hex(&mut token, &payload);
        write_hex(&mut token, mac.as_ref());
        token
    }

    pub fn verify(&self, token: &str, now: SystemTime) -> bool {
        if token.len() != TOKEN_CHARS {
            return false;
        }
        let mut raw = [0u8; PAYLOAD_BYTES + MAC_BYTES];
        if !decode_hex(token, &mut raw) {
            return false;
        }
        let (payload, mac) = raw.split_at(PAYLOAD_BYTES);
        if payload[0] != TOKEN_VERSION {
            return false;
        }
        if hmac::verify(&self.key, payload, mac).is_err() {
            return false;
        }
        let mut seconds = [0u8; 8];
        seconds.copy_from_slice(&payload[1..9]);
        let expiry = UNIX_EPOCH + Duration::from_secs(u64::from_be_bytes(seconds));
        now < expiry
    }
}

pub fn load_secret(data_dir: &Path) -> Option<SessionSecret> {
    let path = data_dir.join(SECRET_FILE);
    crate::password::discard_stray_tmp(&data_dir.join(SECRET_TMP_FILE));

    let text = fs::read_to_string(&path).ok()?;
    let mut bytes = [0u8; SECRET_BYTES];
    if !decode_hex(text.trim(), &mut bytes) {
        if !text.trim().is_empty() {
            tracing::warn!(
                path = %path.display(),
                "session secret is unreadable — regenerating; every session ends"
            );
        }
        return None;
    }
    Some(SessionSecret::from_bytes(&bytes))
}

pub fn write_fresh_secret(data_dir: &Path) -> io::Result<SessionSecret> {
    let mut bytes = [0u8; SECRET_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    let mut hex = String::with_capacity(SECRET_BYTES * 2);
    write_hex(&mut hex, &bytes);
    crate::password::stage_write(
        &data_dir.join(SECRET_FILE),
        &data_dir.join(SECRET_TMP_FILE),
        &hex,
    )?;
    Ok(SessionSecret::from_bytes(&bytes))
}

pub fn set_cookie(token: &str) -> String {
    format!(
        "{COOKIE_NAME}={token}; Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        SESSION_LIFETIME.as_secs()
    )
}

pub fn clear_cookie() -> String {
    format!("{COOKIE_NAME}=; Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

pub fn token_from_cookies(header: &str) -> Option<&str> {
    header.split(';').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name.trim() == COOKIE_NAME).then(|| value.trim())
    })
}

fn write_hex(out: &mut String, bytes: &[u8]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
}

fn decode_hex(text: &str, out: &mut [u8]) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != out.len() * 2 {
        return false;
    }
    for (slot, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
        let (Some(high), Some(low)) = (nibble(pair[0]), nibble(pair[1])) else {
            return false;
        };
        *slot = (high << 4) | low;
    }
    true
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> SessionSecret {
        SessionSecret::from_bytes(&[7u8; SECRET_BYTES])
    }

    fn future() -> SystemTime {
        SystemTime::now() + SESSION_LIFETIME
    }

    #[test]
    fn a_freshly_minted_token_verifies_and_is_the_documented_width() {
        let secret = secret();
        let token = secret.mint(future());
        assert_eq!(token.len(), TOKEN_CHARS);
        assert_eq!(TOKEN_CHARS, 114);
        assert!(secret.verify(&token, SystemTime::now()));
    }

    #[test]
    fn every_token_carries_a_fresh_nonce() {
        let secret = secret();
        let expiry = future();
        assert_ne!(secret.mint(expiry), secret.mint(expiry));
    }

    #[test]
    fn an_expired_token_is_rejected_from_its_own_payload() {
        let secret = secret();
        let token = secret.mint(SystemTime::now() - Duration::from_secs(1));
        assert!(!secret.verify(&token, SystemTime::now()));
    }

    #[test]
    fn a_flipped_payload_byte_fails_the_mac() {
        let secret = secret();
        let token = secret.mint(future());
        let mut bytes = token.into_bytes();
        bytes[20] = if bytes[20] == b'a' { b'b' } else { b'a' };
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(!secret.verify(&tampered, SystemTime::now()));
    }

    #[test]
    fn an_unknown_version_byte_is_rejected_even_with_a_valid_mac() {
        let secret = secret();
        let mut payload = [0u8; PAYLOAD_BYTES];
        payload[0] = TOKEN_VERSION + 1;
        payload[1..9].copy_from_slice(
            &(SystemTime::now() + SESSION_LIFETIME)
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_be_bytes(),
        );
        let mac = hmac::sign(&secret.key, &payload);
        let mut token = String::new();
        write_hex(&mut token, &payload);
        write_hex(&mut token, mac.as_ref());
        assert_eq!(token.len(), TOKEN_CHARS);
        assert!(!secret.verify(&token, SystemTime::now()));
    }

    #[test]
    fn a_token_from_another_secret_never_verifies() {
        let token = secret().mint(future());
        let other = SessionSecret::from_bytes(&[9u8; SECRET_BYTES]);
        assert!(!other.verify(&token, SystemTime::now()));
    }

    #[test]
    fn malformed_tokens_are_rejected_without_panicking() {
        let secret = secret();
        assert!(!secret.verify("", SystemTime::now()));
        assert!(!secret.verify(&"z".repeat(TOKEN_CHARS), SystemTime::now()));
        assert!(!secret.verify(&"a".repeat(TOKEN_CHARS - 1), SystemTime::now()));
    }

    #[test]
    fn the_secret_never_prints_itself() {
        let secret = secret();
        assert_eq!(format!("{secret:?}"), "SessionSecret(redacted)");
        assert_eq!(format!("{secret}"), "<redacted>");
    }

    #[test]
    fn the_cookie_carries_every_required_attribute() {
        let cookie = set_cookie("abc");
        for attribute in ["Secure", "HttpOnly", "SameSite=Strict", "Path=/"] {
            assert!(cookie.contains(attribute), "{cookie}");
        }
        assert!(cookie.starts_with("__Host-fah_session=abc;"));
        assert!(clear_cookie().contains("Max-Age=0"));
    }

    #[test]
    fn the_cookie_is_found_beside_others() {
        assert_eq!(
            token_from_cookies("a=1; __Host-fah_session=tok; b=2"),
            Some("tok")
        );
        assert_eq!(token_from_cookies("__Host-fah_session=tok"), Some("tok"));
        assert_eq!(token_from_cookies("other=1"), None);
        assert_eq!(token_from_cookies(""), None);
    }

    #[test]
    fn a_secret_survives_a_round_trip_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let secret = write_fresh_secret(dir.path()).unwrap();
        let token = secret.mint(future());

        let reloaded = load_secret(dir.path()).expect("the file is readable");
        assert!(reloaded.verify(&token, SystemTime::now()));

        let replaced = write_fresh_secret(dir.path()).unwrap();
        assert!(!replaced.verify(&token, SystemTime::now()));
    }

    #[test]
    fn an_unreadable_secret_reads_as_absent_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(SECRET_FILE), "not hex").unwrap();
        assert!(load_secret(dir.path()).is_none());

        fs::write(dir.path().join(SECRET_FILE), "").unwrap();
        assert!(load_secret(dir.path()).is_none());
    }

    #[test]
    fn a_stray_tmp_beside_the_secret_is_discarded_not_adopted() {
        let dir = tempfile::tempdir().unwrap();
        let secret = write_fresh_secret(dir.path()).unwrap();
        let token = secret.mint(future());
        fs::write(dir.path().join(SECRET_TMP_FILE), "00".repeat(SECRET_BYTES)).unwrap();

        let reloaded = load_secret(dir.path()).expect("the live file is still authoritative");
        assert!(reloaded.verify(&token, SystemTime::now()));
        assert!(!dir.path().join(SECRET_TMP_FILE).exists());
    }
}

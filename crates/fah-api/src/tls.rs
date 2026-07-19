//! TLS for the API (SECURITY.md §TLS for the API): HTTPS by default, a
//! self-signed rcgen certificate generated on first boot into `/config` and
//! stable across restarts, replaceable by dropping your own PEM pair in.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;

const CERT_FILE: &str = "api-cert.pem";
const KEY_FILE: &str = "api-key.pem";

/// The subject names the generated certificate covers. An appliance is
/// reached by IP far more often than by name, so the LAN-facing IP forms are
/// included alongside the hostname.
const SAN_NAMES: [&str; 2] = ["fastadhunter", "localhost"];

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("generating self-signed certificate: {0}")]
    Generate(#[source] rcgen::Error),
    #[error("{path} contains no {what}")]
    Empty { path: PathBuf, what: &'static str },
    #[error("building TLS config: {0}")]
    Config(#[source] rustls::Error),
}

/// Installs the process-wide crypto provider. rustls 0.23 refuses to build a
/// config when more than one provider is compiled in and none is chosen —
/// and this workspace links aws-lc-rs (hickory-net, reqwest) while test-only
/// dependencies can pull ring in. Idempotent: a second call, or one racing
/// another component's install, is not an error.
pub fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

/// Loads the certificate pair from `config_dir`, generating a self-signed one
/// on first boot. A user-supplied PEM pair at the same paths is honored
/// untouched (SECURITY.md: "Users can replace it with their own certificate").
pub fn load_or_generate(config_dir: &Path) -> Result<Arc<ServerConfig>, TlsError> {
    install_crypto_provider();

    let cert_path = config_dir.join(CERT_FILE);
    let key_path = config_dir.join(KEY_FILE);

    let (cert_pem, key_pem) = if cert_path.exists() && key_path.exists() {
        (read(&cert_path)?, read(&key_path)?)
    } else {
        let generated = generate()?;
        write(&cert_path, &generated.cert_pem)?;
        write(&key_path, &generated.key_pem)?;
        restrict_permissions(&key_path)?;
        (generated.cert_pem, generated.key_pem)
    };

    server_config(&cert_pem, &key_pem, &cert_path, &key_path)
}

struct Generated {
    cert_pem: String,
    key_pem: String,
}

fn generate() -> Result<Generated, TlsError> {
    let mut params = rcgen::CertificateParams::new(
        SAN_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>(),
    )
    .map_err(TlsError::Generate)?;
    params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
            std::net::Ipv4Addr::LOCALHOST,
        )));
    params.distinguished_name = {
        let mut dn = rcgen::DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, "FastAdHunter");
        dn
    };

    let key_pair = rcgen::KeyPair::generate().map_err(TlsError::Generate)?;
    let cert = params.self_signed(&key_pair).map_err(TlsError::Generate)?;
    Ok(Generated {
        cert_pem: cert.pem(),
        key_pem: key_pair.serialize_pem(),
    })
}

fn server_config(
    cert_pem: &str,
    key_pem: &str,
    cert_path: &Path,
    key_path: &Path,
) -> Result<Arc<ServerConfig>, TlsError> {
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_pem.as_bytes())
        .collect::<Result<_, _>>()
        .map_err(|source| TlsError::Io {
            path: cert_path.to_path_buf(),
            source,
        })?;
    if certs.is_empty() {
        return Err(TlsError::Empty {
            path: cert_path.to_path_buf(),
            what: "certificate",
        });
    }

    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut key_pem.as_bytes())
        .map_err(|source| TlsError::Io {
            path: key_path.to_path_buf(),
            source,
        })?
        .ok_or_else(|| TlsError::Empty {
            path: key_path.to_path_buf(),
            what: "private key",
        })?;

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(TlsError::Config)?;
    // Advertise HTTP/1.1 and h2: hyper's auto builder serves either, and a
    // browser dashboard negotiating h2 avoids head-of-line blocking on the
    // event stream.
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

fn read(path: &Path) -> Result<String, TlsError> {
    fs::read_to_string(path).map_err(|source| TlsError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn write(path: &Path, text: &str) -> Result<(), TlsError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| TlsError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    fs::write(path, text).map_err(|source| TlsError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), TlsError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| TlsError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<(), TlsError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_boot_generates_a_pair_that_persists_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_or_generate(dir.path()).is_ok());

        let cert = fs::read_to_string(dir.path().join(CERT_FILE)).unwrap();
        let key = fs::read_to_string(dir.path().join(KEY_FILE)).unwrap();
        assert!(cert.contains("BEGIN CERTIFICATE"));
        assert!(key.contains("PRIVATE KEY"));

        // A restart must reuse the same certificate — SECURITY.md promises it
        // is "stable across restarts" so the browser warning is one-time.
        assert!(load_or_generate(dir.path()).is_ok());
        assert_eq!(
            fs::read_to_string(dir.path().join(CERT_FILE)).unwrap(),
            cert
        );
        assert_eq!(fs::read_to_string(dir.path().join(KEY_FILE)).unwrap(), key);
    }

    #[test]
    fn a_user_supplied_pair_is_used_instead_of_generating() {
        let source = tempfile::tempdir().unwrap();
        load_or_generate(source.path()).unwrap();
        let cert = fs::read_to_string(source.path().join(CERT_FILE)).unwrap();
        let key = fs::read_to_string(source.path().join(KEY_FILE)).unwrap();

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CERT_FILE), &cert).unwrap();
        fs::write(dir.path().join(KEY_FILE), &key).unwrap();

        assert!(load_or_generate(dir.path()).is_ok());
        assert_eq!(
            fs::read_to_string(dir.path().join(CERT_FILE)).unwrap(),
            cert,
            "a replacement certificate must be left untouched"
        );
    }

    #[test]
    fn a_malformed_certificate_file_is_reported_not_silently_replaced() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CERT_FILE), "not a pem").unwrap();
        fs::write(dir.path().join(KEY_FILE), "not a pem either").unwrap();

        assert!(matches!(
            load_or_generate(dir.path()),
            Err(TlsError::Empty { .. })
        ));
    }
}

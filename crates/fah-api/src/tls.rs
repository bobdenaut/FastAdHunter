//! TLS for the API (SECURITY.md §TLS for the API): HTTPS by default, a
//! self-signed rcgen certificate generated on first boot into `/config` and
//! stable across restarts, replaceable by dropping your own PEM pair in.

use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;

const CERT_FILE: &str = "api-cert.pem";
const KEY_FILE: &str = "api-key.pem";
const CERT_TMP_FILE: &str = "api-cert.pem.tmp";
const KEY_TMP_FILE: &str = "api-key.pem.tmp";

const VALIDITY_DAYS: i64 = 397;
const CLOCK_SKEW_HOURS: i64 = 1;

/// The subject names the generated certificate covers. An appliance is
/// reached by IP far more often than by name, so the LAN-facing IP forms are
/// included alongside the hostname.
const SAN_NAMES: [&str; 2] = ["fastadhunter", "localhost"];

const SAN_BASELINE_IPS: [IpAddr; 2] = [
    IpAddr::V4(Ipv4Addr::LOCALHOST),
    IpAddr::V6(Ipv6Addr::LOCALHOST),
];

const PROBE_TARGET: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 1);

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
    #[error("building TLS config from {cert} and {key}: {source}")]
    Config {
        cert: PathBuf,
        key: PathBuf,
        #[source]
        source: Box<rustls::Error>,
    },
    #[error(
        "{present} exists but {missing} does not; \
         move both aside together to regenerate the pair"
    )]
    IncompletePair { present: PathBuf, missing: PathBuf },
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
pub fn load_or_generate(
    config_dir: &Path,
    bind_address: &str,
    detected: Option<IpAddr>,
) -> Result<Arc<ServerConfig>, TlsError> {
    install_crypto_provider();

    let cert_path = config_dir.join(CERT_FILE);
    let key_path = config_dir.join(KEY_FILE);
    let cert_tmp = config_dir.join(CERT_TMP_FILE);
    let key_tmp = config_dir.join(KEY_TMP_FILE);

    if cert_path.exists() && !key_path.exists() && key_tmp.exists() {
        tracing::warn!(
            key = %key_path.display(),
            "completing an interrupted certificate generation"
        );
        rename(&key_tmp, &key_path)?;
    }

    let (cert_pem, key_pem) = match (cert_path.exists(), key_path.exists()) {
        (true, true) => {
            discard(&cert_tmp);
            discard(&key_tmp);
            (read(&cert_path)?, read(&key_path)?)
        }
        (true, false) => {
            return Err(TlsError::IncompletePair {
                present: cert_path,
                missing: key_path,
            })
        }
        (false, true) => {
            return Err(TlsError::IncompletePair {
                present: key_path,
                missing: cert_path,
            })
        }
        (false, false) => {
            let generated = generate(bind_address, detected)?;
            write_pair(&generated, &cert_path, &key_path, &cert_tmp, &key_tmp)?;
            (generated.cert_pem, generated.key_pem)
        }
    };

    server_config(&cert_pem, &key_pem, &cert_path, &key_path)
}

struct Generated {
    cert_pem: String,
    key_pem: String,
}

pub fn probe_local_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(PROBE_TARGET).ok()?;
    let local = socket.local_addr().ok()?.ip();
    (!local.is_loopback() && !local.is_unspecified()).then_some(local)
}

fn san_entries(bind_address: &str, detected: Option<IpAddr>) -> (Vec<String>, Vec<IpAddr>) {
    let dns_names = SAN_NAMES
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();

    let mut ip_addresses = SAN_BASELINE_IPS.to_vec();
    let mut add = |ip: IpAddr| {
        if !ip.is_unspecified() && !ip_addresses.contains(&ip) {
            ip_addresses.push(ip);
        }
    };
    if let Ok(bind) = bind_address.parse::<IpAddr>() {
        add(bind);
    }
    if let Some(detected) = detected {
        add(detected);
    }

    (dns_names, ip_addresses)
}

fn generate(bind_address: &str, detected: Option<IpAddr>) -> Result<Generated, TlsError> {
    let (dns_names, ip_addresses) = san_entries(bind_address, detected);

    let mut params =
        rcgen::CertificateParams::new(dns_names.clone()).map_err(TlsError::Generate)?;
    for ip in &ip_addresses {
        params
            .subject_alt_names
            .push(rcgen::SanType::IpAddress(*ip));
    }
    params.distinguished_name = {
        let mut dn = rcgen::DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, "FastAdHunter");
        dn
    };
    params.not_before = time::OffsetDateTime::now_utc() - time::Duration::hours(CLOCK_SKEW_HOURS);
    params.not_after = params.not_before + time::Duration::days(VALIDITY_DAYS);

    let not_after = params.not_after;
    let key_pair = rcgen::KeyPair::generate().map_err(TlsError::Generate)?;
    let cert = params.self_signed(&key_pair).map_err(TlsError::Generate)?;
    tracing::info!(
        dns = ?dns_names,
        ip = ?ip_addresses,
        ?not_after,
        "generated self-signed API certificate"
    );
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
        .map_err(|source| TlsError::Config {
            cert: cert_path.to_path_buf(),
            key: key_path.to_path_buf(),
            source: Box::new(source),
        })?;
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

fn rename(from: &Path, to: &Path) -> Result<(), TlsError> {
    fs::rename(from, to).map_err(|source| TlsError::Io {
        path: to.to_path_buf(),
        source,
    })
}

fn discard(path: &Path) {
    let _ = fs::remove_file(path);
}

fn write_pair(
    generated: &Generated,
    cert_path: &Path,
    key_path: &Path,
    cert_tmp: &Path,
    key_tmp: &Path,
) -> Result<(), TlsError> {
    let staged = write(key_tmp, &generated.key_pem)
        .and_then(|()| restrict_permissions(key_tmp))
        .and_then(|()| write(cert_tmp, &generated.cert_pem))
        .and_then(|()| rename(cert_tmp, cert_path));
    if let Err(error) = staged {
        discard(cert_tmp);
        discard(key_tmp);
        return Err(error);
    }

    if let Err(error) = rename(key_tmp, key_path) {
        discard(cert_path);
        discard(key_tmp);
        return Err(error);
    }
    Ok(())
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

    const BIND: &str = "0.0.0.0";

    fn parsed_sans(cert_pem: &str) -> (Vec<String>, Vec<IpAddr>) {
        let der = rustls_pemfile::certs(&mut cert_pem.as_bytes())
            .next()
            .unwrap()
            .unwrap();
        let (_, cert) = x509_parser::parse_x509_certificate(&der).unwrap();
        let extension = cert.subject_alternative_name().unwrap().unwrap();

        let mut dns_names = Vec::new();
        let mut ip_addresses = Vec::new();
        for name in &extension.value.general_names {
            match name {
                x509_parser::extensions::GeneralName::DNSName(name) => {
                    dns_names.push((*name).to_string())
                }
                x509_parser::extensions::GeneralName::IPAddress(octets) => match octets.len() {
                    4 => {
                        let octets: [u8; 4] = (*octets).try_into().unwrap();
                        ip_addresses.push(IpAddr::from(octets));
                    }
                    16 => {
                        let octets: [u8; 16] = (*octets).try_into().unwrap();
                        ip_addresses.push(IpAddr::from(octets));
                    }
                    _ => panic!("unexpected IP SAN length"),
                },
                other => panic!("unexpected SAN {other:?}"),
            }
        }
        (dns_names, ip_addresses)
    }

    #[test]
    fn the_expected_baseline_san_set_is_four_entries() {
        let (dns_names, ip_addresses) = san_entries(BIND, None);
        assert_eq!(dns_names, ["fastadhunter", "localhost"]);
        assert_eq!(
            ip_addresses,
            [
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V6(Ipv6Addr::LOCALHOST)
            ]
        );
    }

    #[test]
    fn an_unspecified_bind_address_contributes_no_san() {
        let baseline = san_entries("0.0.0.0", None).1;
        assert_eq!(san_entries("::", None).1, baseline);
        assert_eq!(san_entries("not an address", None).1, baseline);
    }

    #[test]
    fn a_literal_bind_address_is_covered() {
        let (_, ip_addresses) = san_entries("192.168.88.1", None);
        assert!(ip_addresses.contains(&IpAddr::V4(Ipv4Addr::new(192, 168, 88, 1))));
    }

    #[test]
    fn a_discovered_address_is_covered_and_deduplicated() {
        let container = IpAddr::V4(Ipv4Addr::new(172, 17, 0, 2));
        let (_, ip_addresses) = san_entries(BIND, Some(container));
        assert_eq!(
            ip_addresses.iter().filter(|ip| **ip == container).count(),
            1
        );

        let (_, ip_addresses) = san_entries("172.17.0.2", Some(container));
        assert_eq!(
            ip_addresses.iter().filter(|ip| **ip == container).count(),
            1
        );
    }

    #[test]
    fn a_loopback_or_unspecified_discovery_adds_nothing() {
        let baseline = san_entries(BIND, None).1;
        for detected in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        ] {
            assert_eq!(san_entries(BIND, Some(detected)).1, baseline);
        }
    }

    #[test]
    fn the_generated_certificate_carries_the_policy_san_set() {
        let detected = IpAddr::V4(Ipv4Addr::new(172, 17, 0, 2));
        let generated = generate("192.168.88.1", Some(detected)).unwrap();

        let (expected_dns, expected_ips) = san_entries("192.168.88.1", Some(detected));
        let (dns_names, ip_addresses) = parsed_sans(&generated.cert_pem);

        assert_eq!(dns_names, expected_dns);
        assert_eq!(ip_addresses, expected_ips);
        assert!(ip_addresses.contains(&IpAddr::V4(Ipv4Addr::new(192, 168, 88, 1))));
        assert!(ip_addresses.contains(&detected));
    }

    #[test]
    fn the_generated_certificate_is_bounded_to_397_days() {
        let generated = generate(BIND, None).unwrap();
        let der = rustls_pemfile::certs(&mut generated.cert_pem.as_bytes())
            .next()
            .unwrap()
            .unwrap();
        let (_, cert) = x509_parser::parse_x509_certificate(&der).unwrap();

        let validity = cert.validity();
        let span = validity.not_after.timestamp() - validity.not_before.timestamp();
        assert_eq!(span, VALIDITY_DAYS * 24 * 60 * 60);

        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        assert!(validity.not_before.timestamp() <= now);
        assert!(validity.not_after.timestamp() > now);
    }

    #[test]
    fn a_half_pair_is_reported_and_the_survivor_is_left_untouched() {
        for (present, missing) in [(CERT_FILE, KEY_FILE), (KEY_FILE, CERT_FILE)] {
            let source = tempfile::tempdir().unwrap();
            load_or_generate(source.path(), BIND, None).unwrap();
            let survivor = fs::read_to_string(source.path().join(present)).unwrap();

            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join(present), &survivor).unwrap();

            assert!(matches!(
                load_or_generate(dir.path(), BIND, None),
                Err(TlsError::IncompletePair { .. })
            ));
            assert_eq!(
                fs::read_to_string(dir.path().join(present)).unwrap(),
                survivor,
                "the surviving half of the pair must not be rewritten"
            );
            assert!(!dir.path().join(missing).exists());
        }
    }

    #[test]
    fn an_interrupted_generation_is_completed_on_the_next_boot() {
        let source = tempfile::tempdir().unwrap();
        load_or_generate(source.path(), BIND, None).unwrap();
        let cert = fs::read_to_string(source.path().join(CERT_FILE)).unwrap();
        let key = fs::read_to_string(source.path().join(KEY_FILE)).unwrap();

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CERT_FILE), &cert).unwrap();
        fs::write(dir.path().join(KEY_TMP_FILE), &key).unwrap();

        assert!(load_or_generate(dir.path(), BIND, None).is_ok());
        assert_eq!(
            fs::read_to_string(dir.path().join(CERT_FILE)).unwrap(),
            cert,
            "the staged certificate must survive the recovery"
        );
        assert_eq!(fs::read_to_string(dir.path().join(KEY_FILE)).unwrap(), key);
        assert!(!dir.path().join(KEY_TMP_FILE).exists());
    }

    #[test]
    fn a_stale_temp_file_is_discarded_once_the_pair_is_complete() {
        let dir = tempfile::tempdir().unwrap();
        load_or_generate(dir.path(), BIND, None).unwrap();
        let key = fs::read_to_string(dir.path().join(KEY_FILE)).unwrap();

        fs::write(dir.path().join(KEY_TMP_FILE), "stale key").unwrap();
        fs::write(dir.path().join(CERT_TMP_FILE), "stale cert").unwrap();

        assert!(load_or_generate(dir.path(), BIND, None).is_ok());
        assert!(!dir.path().join(KEY_TMP_FILE).exists());
        assert!(!dir.path().join(CERT_TMP_FILE).exists());
        assert_eq!(fs::read_to_string(dir.path().join(KEY_FILE)).unwrap(), key);
    }

    #[test]
    fn a_half_pair_without_a_staged_survivor_is_still_reported() {
        let source = tempfile::tempdir().unwrap();
        load_or_generate(source.path(), BIND, None).unwrap();
        let key = fs::read_to_string(source.path().join(KEY_FILE)).unwrap();

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(KEY_FILE), &key).unwrap();
        fs::write(
            dir.path().join(CERT_TMP_FILE),
            "staged in the wrong direction",
        )
        .unwrap();

        assert!(matches!(
            load_or_generate(dir.path(), BIND, None),
            Err(TlsError::IncompletePair { .. })
        ));
        assert_eq!(fs::read_to_string(dir.path().join(KEY_FILE)).unwrap(), key);
    }

    #[test]
    fn first_boot_generates_a_pair_that_persists_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_or_generate(dir.path(), BIND, None).is_ok());

        let cert = fs::read_to_string(dir.path().join(CERT_FILE)).unwrap();
        let key = fs::read_to_string(dir.path().join(KEY_FILE)).unwrap();
        assert!(cert.contains("BEGIN CERTIFICATE"));
        assert!(key.contains("PRIVATE KEY"));

        // A restart must reuse the same certificate — SECURITY.md promises it
        // is "stable across restarts" so the browser warning is one-time.
        assert!(load_or_generate(dir.path(), BIND, None).is_ok());
        assert_eq!(
            fs::read_to_string(dir.path().join(CERT_FILE)).unwrap(),
            cert
        );
        assert_eq!(fs::read_to_string(dir.path().join(KEY_FILE)).unwrap(), key);
    }

    #[test]
    fn a_user_supplied_pair_is_used_instead_of_generating() {
        let source = tempfile::tempdir().unwrap();
        load_or_generate(source.path(), BIND, None).unwrap();
        let cert = fs::read_to_string(source.path().join(CERT_FILE)).unwrap();
        let key = fs::read_to_string(source.path().join(KEY_FILE)).unwrap();

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CERT_FILE), &cert).unwrap();
        fs::write(dir.path().join(KEY_FILE), &key).unwrap();

        assert!(load_or_generate(dir.path(), BIND, None).is_ok());
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
            load_or_generate(dir.path(), BIND, None),
            Err(TlsError::Empty { .. })
        ));
    }

    #[test]
    fn a_literal_bind_address_reaches_the_generated_certificate() {
        let dir = tempfile::tempdir().unwrap();
        load_or_generate(dir.path(), "192.168.88.1", None).unwrap();

        let cert_pem = fs::read_to_string(dir.path().join(CERT_FILE)).unwrap();
        let (dns_names, ip_addresses) = parsed_sans(&cert_pem);

        assert_eq!(dns_names, ["fastadhunter", "localhost"]);
        assert!(
            ip_addresses.contains(&IpAddr::V4(Ipv4Addr::new(192, 168, 88, 1))),
            "the bind address the caller passed must reach the certificate"
        );
        assert_eq!(ip_addresses, san_entries("192.168.88.1", None).1);
    }

    #[tokio::test]
    async fn the_generated_certificate_passes_the_name_check_for_its_sans() {
        let dir = tempfile::tempdir().unwrap();
        let server = load_or_generate(dir.path(), BIND, None).unwrap();

        let cert_pem = fs::read_to_string(dir.path().join(CERT_FILE)).unwrap();
        let der = rustls_pemfile::certs(&mut cert_pem.as_bytes())
            .next()
            .unwrap()
            .unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(der).unwrap();
        let client = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );

        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(server);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let _ = acceptor.accept(stream).await;
                });
            }
        });

        let covered =
            rustls::pki_types::ServerName::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST).into());
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        tokio_rustls::TlsConnector::from(Arc::clone(&client))
            .connect(covered, stream)
            .await
            .expect("an address in the SAN set must pass the name check");

        let uncovered = rustls::pki_types::ServerName::try_from("not-fastadhunter").unwrap();
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let error = tokio_rustls::TlsConnector::from(client)
            .connect(uncovered, stream)
            .await
            .expect_err("a name outside the SAN set must fail the name check");
        let error = error.to_string();
        assert!(
            error.contains("NotValidForName") || error.to_lowercase().contains("not valid for"),
            "expected a name-check failure, got: {error}"
        );
    }
}

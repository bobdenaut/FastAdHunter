use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use rustls::pki_types::PrivateKeyDer;

use crate::error::CertError;
use crate::store::{all_certificates, certificate_pem};

struct Material {
    cert_pem: String,
    key_pem: String,
}

impl Material {
    fn debug(&self, name: &'static str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(name)
            .field("cert_pem", &self.cert_pem)
            .field("key_pem", &"<redacted>")
            .finish()
    }
}

pub struct ValidatedServerPair(Material);

pub struct ValidatedCaPair(Material);

impl fmt::Debug for ValidatedServerPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.debug("ValidatedServerPair", f)
    }
}

impl fmt::Debug for ValidatedCaPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.debug("ValidatedCaPair", f)
    }
}

impl ValidatedServerPair {
    pub fn cert_pem(&self) -> &str {
        &self.0.cert_pem
    }

    pub(crate) fn key_pem(&self) -> &str {
        &self.0.key_pem
    }
}

impl ValidatedCaPair {
    pub fn cert_pem(&self) -> &str {
        &self.0.cert_pem
    }

    pub(crate) fn key_pem(&self) -> &str {
        &self.0.key_pem
    }
}

pub fn validate_server_pair(
    cert_pem: &str,
    key_pem: &str,
) -> Result<ValidatedServerPair, CertError> {
    validate(cert_pem, key_pem, false).map(ValidatedServerPair)
}

pub fn validate_ca_pair(cert_pem: &str, key_pem: &str) -> Result<ValidatedCaPair, CertError> {
    validate(cert_pem, key_pem, true).map(ValidatedCaPair)
}

fn validate(cert_pem: &str, key_pem: &str, require_ca: bool) -> Result<Material, CertError> {
    let chain = all_certificates(cert_pem, "the certificate")?;
    let (_, parsed) = x509_parser::parse_x509_certificate(&chain[0])
        .map_err(|error| CertError::parse("the certificate", error))?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default();
    let not_before = parsed.validity().not_before.timestamp();
    let not_after = parsed.validity().not_after.timestamp();
    if now < not_before {
        return Err(CertError::NotYetValid { not_before });
    }
    if now > not_after {
        return Err(CertError::Expired { not_after });
    }

    if require_ca && !signs_certificates(&parsed) {
        return Err(CertError::NotACa);
    }

    ensure_key_matches(key_pem, parsed.tbs_certificate.subject_pki.raw)?;

    let certificates = match require_ca {
        true => &chain[..1],
        false => &chain[..],
    };
    Ok(Material {
        cert_pem: certificate_pem(certificates),
        key_pem: key_pem.to_string(),
    })
}

pub(crate) fn ensure_key_matches(key_pem: &str, subject_pki: &[u8]) -> Result<(), CertError> {
    let key = private_key(key_pem)?;
    let signing_key = rustls::crypto::aws_lc_rs::sign::any_supported_type(&key)
        .map_err(|error| CertError::parse("the private key", error))?;
    let Some(spki) = signing_key.public_key() else {
        return Err(CertError::KeyMismatch);
    };
    match spki.as_ref() == subject_pki {
        true => Ok(()),
        false => Err(CertError::KeyMismatch),
    }
}

fn signs_certificates(parsed: &x509_parser::certificate::X509Certificate<'_>) -> bool {
    if !parsed.is_ca() {
        return false;
    }
    match parsed.key_usage() {
        Ok(Some(usage)) => usage.value.key_cert_sign(),
        _ => true,
    }
}

fn private_key(key_pem: &str) -> Result<PrivateKeyDer<'static>, CertError> {
    match rustls_pemfile::private_key(&mut key_pem.as_bytes()) {
        Ok(Some(key)) => Ok(key),
        Ok(None) => Err(CertError::parse("the private key", "no PRIVATE KEY block")),
        Err(error) => Err(CertError::parse("the private key", error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Pair {
        cert_pem: String,
        key_pem: String,
    }

    fn issue(is_ca: bool, offset_days: i64, validity_days: i64) -> Pair {
        crate::install_crypto_provider();
        let mut params = rcgen::CertificateParams::new(vec!["import.example".to_string()]).unwrap();
        if is_ca {
            params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            params.key_usages = vec![
                rcgen::KeyUsagePurpose::KeyCertSign,
                rcgen::KeyUsagePurpose::CrlSign,
            ];
        }
        params.not_before = time::OffsetDateTime::now_utc() + time::Duration::days(offset_days);
        params.not_after = params.not_before + time::Duration::days(validity_days);

        let key_pair = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key_pair).unwrap();
        Pair {
            cert_pem: cert.pem(),
            key_pem: key_pair.serialize_pem(),
        }
    }

    fn valid_leaf() -> Pair {
        issue(false, -1, 30)
    }

    fn from_key_pair(
        key_pair: rcgen::KeyPair,
        is_ca: bool,
        edit: impl FnOnce(&mut rcgen::CertificateParams),
    ) -> Pair {
        crate::install_crypto_provider();
        let mut params = rcgen::CertificateParams::new(vec!["import.example".to_string()]).unwrap();
        if is_ca {
            params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        }
        params.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(1);
        params.not_after = params.not_before + time::Duration::days(30);
        edit(&mut params);

        let cert = params.self_signed(&key_pair).unwrap();
        Pair {
            cert_pem: cert.pem(),
            key_pem: key_pair.serialize_pem(),
        }
    }

    fn issue_for(algorithm: &'static rcgen::SignatureAlgorithm) -> Pair {
        from_key_pair(
            rcgen::KeyPair::generate_for(algorithm).unwrap(),
            false,
            |_| {},
        )
    }

    fn issue_rsa() -> Pair {
        let key_pair =
            rcgen::KeyPair::generate_rsa_for(&rcgen::PKCS_RSA_SHA256, rcgen::RsaKeySize::_2048)
                .unwrap();
        from_key_pair(key_pair, false, |_| {})
    }

    fn issue_with(is_ca: bool, edit: impl FnOnce(&mut rcgen::CertificateParams)) -> Pair {
        from_key_pair(rcgen::KeyPair::generate().unwrap(), is_ca, edit)
    }

    #[test]
    fn a_valid_server_pair_is_accepted_and_redacts_its_key() {
        let pair = valid_leaf();
        let imported = validate_server_pair(&pair.cert_pem, &pair.key_pem).unwrap();
        assert_eq!(
            all_certificates(imported.cert_pem(), "the certificate").unwrap(),
            all_certificates(&pair.cert_pem, "the certificate").unwrap()
        );
        assert_eq!(imported.key_pem(), pair.key_pem);

        let rendered = format!("{imported:?}");
        assert!(!rendered.contains("PRIVATE KEY"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn garbage_is_rejected_as_a_parse_failure() {
        assert!(matches!(
            validate_server_pair("not a pem", "not a key"),
            Err(CertError::Parse { .. })
        ));
        let pair = valid_leaf();
        assert!(matches!(
            validate_server_pair(&pair.cert_pem, "not a key"),
            Err(CertError::Parse { .. })
        ));
    }

    #[test]
    fn an_expired_certificate_is_rejected_by_name() {
        let pair = issue(false, -60, 30);
        assert!(matches!(
            validate_server_pair(&pair.cert_pem, &pair.key_pem),
            Err(CertError::Expired { .. })
        ));
    }

    #[test]
    fn a_not_yet_valid_certificate_is_rejected_by_name() {
        let pair = issue(false, 30, 30);
        assert!(matches!(
            validate_server_pair(&pair.cert_pem, &pair.key_pem),
            Err(CertError::NotYetValid { .. })
        ));
    }

    #[test]
    fn a_key_from_another_pair_is_rejected_as_a_mismatch() {
        let pair = valid_leaf();
        let other = valid_leaf();
        assert!(matches!(
            validate_server_pair(&pair.cert_pem, &other.key_pem),
            Err(CertError::KeyMismatch)
        ));
    }

    #[test]
    fn a_leaf_where_a_ca_is_expected_is_rejected_as_not_a_ca() {
        let leaf = valid_leaf();
        assert!(matches!(
            validate_ca_pair(&leaf.cert_pem, &leaf.key_pem),
            Err(CertError::NotACa)
        ));

        let authority = issue(true, -1, 30);
        assert!(validate_ca_pair(&authority.cert_pem, &authority.key_pem).is_ok());
        assert!(
            validate_server_pair(&leaf.cert_pem, &leaf.key_pem).is_ok(),
            "the API server pair path must not require CA-ness"
        );
    }

    #[test]
    fn the_key_match_check_runs_for_every_key_algorithm_the_validator_accepts() {
        let algorithms: [&'static rcgen::SignatureAlgorithm; 3] = [
            &rcgen::PKCS_ECDSA_P256_SHA256,
            &rcgen::PKCS_ECDSA_P384_SHA384,
            &rcgen::PKCS_ED25519,
        ];
        for algorithm in algorithms {
            let pair = issue_for(algorithm);
            let other = issue_for(algorithm);
            assert!(
                validate_server_pair(&pair.cert_pem, &pair.key_pem).is_ok(),
                "a matching pair must be accepted"
            );
            assert!(
                matches!(
                    validate_server_pair(&pair.cert_pem, &other.key_pem),
                    Err(CertError::KeyMismatch)
                ),
                "a mismatched key must never be accepted by omission of the check"
            );
        }
    }

    #[test]
    fn an_rsa_pair_is_key_matched_rather_than_waved_through() {
        let pair = issue_rsa();
        let other = issue_rsa();
        assert!(validate_server_pair(&pair.cert_pem, &pair.key_pem).is_ok());
        assert!(matches!(
            validate_server_pair(&pair.cert_pem, &other.key_pem),
            Err(CertError::KeyMismatch)
        ));
    }

    #[test]
    fn an_authority_that_may_not_sign_certificates_is_rejected() {
        let unable = issue_with(true, |params| {
            params.key_usages = vec![rcgen::KeyUsagePurpose::CrlSign];
        });
        assert!(matches!(
            validate_ca_pair(&unable.cert_pem, &unable.key_pem),
            Err(CertError::NotACa)
        ));

        let unrestricted = issue_with(true, |params| {
            params.key_usages = Vec::new();
        });
        assert!(
            validate_ca_pair(&unrestricted.cert_pem, &unrestricted.key_pem).is_ok(),
            "an absent keyUsage extension places no restriction (RFC 5280)"
        );
    }

    #[test]
    fn private_material_pasted_into_the_certificate_field_is_dropped() {
        let authority = issue(true, -1, 30);
        let blob = format!("{}{}", authority.cert_pem, authority.key_pem);

        let ca = validate_ca_pair(&blob, &authority.key_pem).unwrap();
        assert!(!ca.cert_pem().contains("PRIVATE"));
        assert_eq!(ca.cert_pem().matches("BEGIN CERTIFICATE").count(), 1);

        let leaf = valid_leaf();
        let leaf_blob = format!("{}{}", leaf.cert_pem, leaf.key_pem);
        let server = validate_server_pair(&leaf_blob, &leaf.key_pem).unwrap();
        assert!(!server.cert_pem().contains("PRIVATE"));
        assert_eq!(server.cert_pem().matches("BEGIN CERTIFICATE").count(), 1);
    }

    #[test]
    fn a_server_chain_keeps_every_certificate_but_a_ca_keeps_only_the_root() {
        let authority = issue(true, -1, 30);
        let leaf = valid_leaf();
        let chain = format!("{}{}", leaf.cert_pem, authority.cert_pem);

        let server = validate_server_pair(&chain, &leaf.key_pem).unwrap();
        assert_eq!(server.cert_pem().matches("BEGIN CERTIFICATE").count(), 2);

        let ca = validate_ca_pair(&authority.cert_pem, &authority.key_pem).unwrap();
        assert_eq!(ca.cert_pem().matches("BEGIN CERTIFICATE").count(), 1);
    }
}

use std::fmt;
use std::fmt::Write as _;

use rustls::pki_types::CertificateDer;

use crate::error::CertError;

pub const DEFAULT_CA_COMMON_NAME: &str = "FastAdHunter CA";
pub const DEFAULT_CA_VALIDITY_DAYS: i64 = 3650;

pub(crate) const CLOCK_SKEW_HOURS: i64 = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaParams {
    pub common_name: String,
    pub validity_days: i64,
}

impl Default for CaParams {
    fn default() -> Self {
        Self {
            common_name: DEFAULT_CA_COMMON_NAME.to_string(),
            validity_days: DEFAULT_CA_VALIDITY_DAYS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaSummary {
    pub fingerprint_sha256: String,
    pub not_before: i64,
    pub not_after: i64,
    pub subject: String,
}

pub(crate) struct Generated {
    pub(crate) cert_pem: String,
    pub(crate) key_pem: String,
}

pub(crate) struct CaHandle {
    issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
    cert_pem: String,
    cert_der: CertificateDer<'static>,
    summary: CaSummary,
}

impl fmt::Debug for CaHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaHandle")
            .field("summary", &self.summary)
            .field("issuer_key", &"<redacted>")
            .finish()
    }
}

impl CaHandle {
    pub(crate) fn load(cert_pem: &str, key_pem: &str) -> Result<Self, CertError> {
        let key = rcgen::KeyPair::from_pem(key_pem)
            .map_err(|error| CertError::parse("the CA private key", error))?;
        let cert_der = crate::store::first_certificate(cert_pem, "the CA certificate")?;
        let summary = summarize(&cert_der, key_pem)?;
        let cert_pem = crate::store::certificate_pem(std::slice::from_ref(&cert_der));
        let issuer = rcgen::Issuer::from_ca_cert_pem(&cert_pem, key)
            .map_err(|error| CertError::parse("the CA certificate", error))?;
        Ok(Self {
            issuer,
            cert_pem,
            cert_der,
            summary,
        })
    }

    pub(crate) fn issuer(&self) -> &rcgen::Issuer<'static, rcgen::KeyPair> {
        &self.issuer
    }

    pub(crate) fn cert_pem(&self) -> &str {
        &self.cert_pem
    }

    pub(crate) fn cert_der(&self) -> &CertificateDer<'static> {
        &self.cert_der
    }

    pub(crate) fn summary(&self) -> &CaSummary {
        &self.summary
    }
}

pub(crate) fn generate(params: &CaParams) -> Result<Generated, CertError> {
    let mut certificate =
        rcgen::CertificateParams::new(Vec::<String>::new()).map_err(CertError::Generate)?;
    certificate.distinguished_name = {
        let mut dn = rcgen::DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, params.common_name.as_str());
        dn
    };
    certificate.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
    certificate.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
        rcgen::KeyUsagePurpose::DigitalSignature,
    ];
    certificate.not_before =
        time::OffsetDateTime::now_utc() - time::Duration::hours(CLOCK_SKEW_HOURS);
    certificate.not_after = certificate.not_before + time::Duration::days(params.validity_days);

    let key_pair = rcgen::KeyPair::generate().map_err(CertError::Generate)?;
    let cert = certificate
        .self_signed(&key_pair)
        .map_err(CertError::Generate)?;
    Ok(Generated {
        cert_pem: cert.pem(),
        key_pem: key_pair.serialize_pem(),
    })
}

pub(crate) fn summarize(der: &CertificateDer<'_>, key_pem: &str) -> Result<CaSummary, CertError> {
    let (_, parsed) = x509_parser::parse_x509_certificate(der)
        .map_err(|error| CertError::parse("the CA certificate", error))?;
    if !parsed.is_ca() {
        return Err(CertError::NotACa);
    }
    crate::import::ensure_key_matches(key_pem, parsed.tbs_certificate.subject_pki.raw)?;
    Ok(CaSummary {
        fingerprint_sha256: fingerprint(der),
        not_before: parsed.validity().not_before.timestamp(),
        not_after: parsed.validity().not_after.timestamp(),
        subject: parsed.subject().to_string(),
    })
}

pub(crate) fn fingerprint(der: &CertificateDer<'_>) -> String {
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, der.as_ref());
    let bytes = digest.as_ref();
    let mut out = String::with_capacity(bytes.len() * 3 - 1);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            out.push(':');
        }
        let _ = write!(out, "{byte:02X}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::first_certificate;

    #[test]
    fn the_default_authority_is_a_ca_bounded_to_its_validity_window() {
        let params = CaParams::default();
        let generated = generate(&params).unwrap();
        let der = first_certificate(&generated.cert_pem, "the CA certificate").unwrap();
        let (_, parsed) = x509_parser::parse_x509_certificate(&der).unwrap();

        assert!(parsed.is_ca());
        let validity = parsed.validity();
        let span = validity.not_after.timestamp() - validity.not_before.timestamp();
        assert_eq!(span, DEFAULT_CA_VALIDITY_DAYS * 24 * 60 * 60);

        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        assert!(
            validity.not_before.timestamp() < now,
            "the CA is backdated so client clock skew cannot make it not-yet-valid"
        );
    }

    #[test]
    fn the_fingerprint_is_stable_for_the_same_der_and_differs_across_authorities() {
        let first = generate(&CaParams::default()).unwrap();
        let second = generate(&CaParams::default()).unwrap();

        let first_der = first_certificate(&first.cert_pem, "the CA certificate").unwrap();
        let second_der = first_certificate(&second.cert_pem, "the CA certificate").unwrap();

        assert_eq!(fingerprint(&first_der), fingerprint(&first_der));
        assert_ne!(fingerprint(&first_der), fingerprint(&second_der));
        assert_eq!(fingerprint(&first_der).len(), 32 * 3 - 1);
    }

    #[test]
    fn a_loaded_authority_reports_what_is_on_disk_and_redacts_its_key() {
        let params = CaParams {
            common_name: "Test Authority".to_string(),
            validity_days: 30,
        };
        let generated = generate(&params).unwrap();
        let handle = CaHandle::load(&generated.cert_pem, &generated.key_pem).unwrap();

        let der = first_certificate(&generated.cert_pem, "the CA certificate").unwrap();
        assert_eq!(handle.summary().fingerprint_sha256, fingerprint(&der));
        assert!(handle.summary().subject.contains("Test Authority"));
        assert_eq!(handle.cert_der(), &der);

        let rendered = format!("{handle:?}");
        assert!(!rendered.contains("PRIVATE KEY"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn a_loaded_authority_re_encodes_its_certificate_instead_of_echoing_the_input() {
        let generated = generate(&CaParams::default()).unwrap();
        let blob = format!("{}{}", generated.cert_pem, generated.key_pem);
        let handle = CaHandle::load(&blob, &generated.key_pem).unwrap();

        assert!(
            !handle.cert_pem().contains("PRIVATE"),
            "the exported PEM is rebuilt from the parsed certificate, never echoed"
        );
        assert_eq!(handle.cert_pem().matches("BEGIN CERTIFICATE").count(), 1);
        assert_eq!(
            first_certificate(handle.cert_pem(), "the CA certificate").unwrap(),
            *handle.cert_der()
        );
    }

    #[test]
    fn an_authority_whose_key_does_not_match_its_certificate_cannot_be_loaded() {
        crate::install_crypto_provider();
        let first = generate(&CaParams::default()).unwrap();
        let second = generate(&CaParams::default()).unwrap();

        assert!(matches!(
            CaHandle::load(&first.cert_pem, &second.key_pem),
            Err(CertError::KeyMismatch)
        ));
        assert!(CaHandle::load(&first.cert_pem, &first.key_pem).is_ok());
    }

    #[test]
    fn a_certificate_that_is_not_an_authority_cannot_be_loaded_as_one() {
        crate::install_crypto_provider();
        let mut params = rcgen::CertificateParams::new(vec!["leaf.example".to_string()]).unwrap();
        params.is_ca = rcgen::IsCa::ExplicitNoCa;
        let key_pair = rcgen::KeyPair::generate().unwrap();
        let leaf = params.self_signed(&key_pair).unwrap();

        assert!(matches!(
            CaHandle::load(&leaf.pem(), &key_pair.serialize_pem()),
            Err(CertError::NotACa)
        ));
    }
}

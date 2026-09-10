#![allow(dead_code)]

use std::sync::Arc;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{CertificateError, SupportedProtocolVersion};
use tokio_rustls::TlsConnector;

pub fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

#[derive(Debug)]
struct Rejecting {
    provider: Arc<rustls::crypto::CryptoProvider>,
    verdict: CertificateError,
}

impl rustls::client::danger::ServerCertVerifier for Rejecting {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Err(rustls::Error::InvalidCertificate(self.verdict.clone()))
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub fn rejecting_connector(
    verdict: CertificateError,
    versions: &[&'static SupportedProtocolVersion],
) -> TlsConnector {
    let provider = provider();
    let config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(versions)
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Rejecting { provider, verdict }))
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

fn untrusting_config() -> Arc<rustls::ClientConfig> {
    let config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
    Arc::new(config)
}

pub fn untrusting_connector() -> TlsConnector {
    TlsConnector::from(untrusting_config())
}

pub fn client_hello(host: &str) -> Vec<u8> {
    let name = ServerName::try_from(host.to_string()).unwrap();
    let mut client = rustls::ClientConnection::new(untrusting_config(), name).unwrap();
    let mut hello = Vec::new();
    client.write_tls(&mut hello).unwrap();
    hello
}

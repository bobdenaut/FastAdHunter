//! Everything that speaks a wire protocol. Nothing above this module holds a
//! URL, a token or a socket.
//!
//! One `api.rs` covers every FastAdHunter endpoint, because a request here is
//! an origin, a bearer token and a path — a file per endpoint would duplicate
//! the request building and error classification for callers that differ only
//! in their path constant.

pub mod api;
pub mod events;
pub mod routeros;

use std::sync::Arc;

use crate::config::Config;
use crate::BoxError;

pub use api::ApiClient;
pub use events::EventsClient;
pub use routeros::RouterOsClient;

/// Every outbound handle the program owns, built once at startup.
pub struct Clients {
    pub api: ApiClient,
    pub events: EventsClient,
    /// `None` when no `[routeros] base_url` is configured — the footer's router
    /// figures are the only thing that depends on it.
    pub routeros: Option<RouterOsClient>,
}

impl Clients {
    pub fn new(config: &Config) -> Result<Self, BoxError> {
        let tls = TlsPolicy::from(config);
        Ok(Self {
            api: ApiClient::new(config, &tls)?,
            events: EventsClient::new(config, &tls)?,
            routeros: config
                .routeros
                .base_url
                .as_deref()
                .map(|base| RouterOsClient::new(base, &config.routeros, config.timeout.routeros()))
                .transpose()?,
        })
    }
}

/// How the two TLS stacks in play — reqwest's and tungstenite's — are told to
/// treat the appliance's certificate. One decision, applied twice, so the HTTP
/// and websocket halves cannot end up trusting different things.
pub struct TlsPolicy {
    accept_invalid_certs: bool,
}

impl From<&Config> for TlsPolicy {
    fn from(config: &Config) -> Self {
        Self {
            accept_invalid_certs: config.accept_invalid_certs,
        }
    }
}

impl TlsPolicy {
    fn apply(&self, builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
        builder.danger_accept_invalid_certs(self.accept_invalid_certs)
    }

    /// A rustls config for the websocket, or `None` to let tungstenite use its
    /// own webpki-roots default.
    fn websocket_connector(&self) -> Option<Arc<rustls::ClientConfig>> {
        if !self.accept_invalid_certs {
            return None;
        }

        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
            .with_safe_default_protocol_versions()
            .ok()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(dangerous::AcceptAnyServerCert(provider)))
            .with_no_client_auth();
        Some(Arc::new(config))
    }
}

/// The verifier behind `accept_invalid_certs`, for an appliance whose
/// certificate is generated at first boot and vouched for by no root store.
///
/// Not hand-rolled crypto (hard rule 5): signature checking is delegated to the
/// provider unchanged and only the identity check is skipped — what
/// `danger_accept_invalid_certs` does for the HTTP half.
mod dangerous {
    use std::sync::Arc;

    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::crypto::CryptoProvider;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, Error, SignatureScheme};

    #[derive(Debug)]
    pub struct AcceptAnyServerCert(pub Arc<CryptoProvider>);

    impl ServerCertVerifier for AcceptAnyServerCert {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            rustls::crypto::verify_tls12_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            rustls::crypto::verify_tls13_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
}

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{AlertDescription, CertificateError};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

mod common;

use common::{provider, rejecting_connector, untrusting_connector};

const ORIGIN_NAME: &str = "origin.test";

fn self_signed() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let key_pair = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec![ORIGIN_NAME.to_string()]).unwrap();
    let cert = params.self_signed(&key_pair).unwrap();
    (
        cert.der().clone(),
        PrivateKeyDer::try_from(key_pair.serialize_der()).unwrap(),
    )
}

fn acceptor() -> TlsAcceptor {
    let (cert, key) = self_signed();
    let config = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    TlsAcceptor::from(Arc::new(config))
}

async fn listener() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    (listener, addr)
}

async fn accept_error(connector: TlsConnector) -> io::Error {
    let (listener, addr) = listener().await;
    let acceptor = acceptor();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        acceptor.accept(stream).await.err()
    });
    let client = tokio::spawn(async move {
        let tcp = TcpStream::connect(addr).await.unwrap();
        let name = ServerName::try_from(ORIGIN_NAME).unwrap();
        let _ = connector.connect(name, tcp).await;
    });
    let error = server.await.unwrap();
    let _ = client.await;
    error.expect("the client rejected our leaf, so accept must fail")
}

fn alert(error: &io::Error) -> Option<AlertDescription> {
    if error.kind() != io::ErrorKind::InvalidData {
        return None;
    }
    match error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        Some(rustls::Error::AlertReceived(description)) => Some(*description),
        _ => None,
    }
}

#[test]
fn the_certificate_alerts_carry_their_wire_values() {
    assert_eq!(u8::from(AlertDescription::BadCertificate), 0x2a);
    assert_eq!(u8::from(AlertDescription::CertificateUnknown), 0x2e);
    assert_eq!(u8::from(AlertDescription::UnknownCA), 0x30);
    assert_eq!(u8::from(AlertDescription::AccessDenied), 0x31);
}

#[tokio::test]
async fn a_client_refusing_our_leaf_reaches_accept_as_a_downcastable_alert() {
    let cases: Vec<(&str, TlsConnector, AlertDescription)> = vec![
        (
            "tls1.3, no trust anchor for our CA",
            untrusting_connector(),
            AlertDescription::UnknownCA,
        ),
        (
            "tls1.3, verifier refuses the identity",
            rejecting_connector(
                CertificateError::ApplicationVerificationFailure,
                rustls::ALL_VERSIONS,
            ),
            AlertDescription::AccessDenied,
        ),
        (
            "tls1.3, verifier cannot build a chain",
            rejecting_connector(CertificateError::UnknownIssuer, rustls::ALL_VERSIONS),
            AlertDescription::UnknownCA,
        ),
        (
            "tls1.3, verifier refuses the name",
            rejecting_connector(CertificateError::NotValidForName, rustls::ALL_VERSIONS),
            AlertDescription::BadCertificate,
        ),
        (
            "tls1.2, verifier refuses the identity",
            rejecting_connector(
                CertificateError::ApplicationVerificationFailure,
                &[&rustls::version::TLS12],
            ),
            AlertDescription::AccessDenied,
        ),
        (
            "tls1.2, verifier cannot build a chain",
            rejecting_connector(CertificateError::UnknownIssuer, &[&rustls::version::TLS12]),
            AlertDescription::UnknownCA,
        ),
    ];

    for (case, connector, expected) in cases {
        let error = accept_error(connector).await;
        assert_eq!(
            error.kind(),
            io::ErrorKind::InvalidData,
            "{case}: {error} ({:?})",
            error.kind()
        );
        assert_eq!(alert(&error), Some(expected), "{case}: {error}");
    }
}

#[tokio::test]
async fn a_pinning_alert_is_not_the_upstream_certificate_failure_shape() {
    let error = accept_error(rejecting_connector(
        CertificateError::ApplicationVerificationFailure,
        rustls::ALL_VERSIONS,
    ))
    .await;
    let inner = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        .expect("a rustls error rides inside the accept failure");
    assert!(
        !matches!(inner, rustls::Error::InvalidCertificate(_)),
        "the accept side reports the alert it received, not a local verification verdict: {inner:?}"
    );
}

#[tokio::test]
async fn a_client_that_closes_without_alerting_carries_nothing_to_classify() {
    let (listener, addr) = listener().await;
    let acceptor = acceptor();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        acceptor.accept(stream).await.err()
    });
    let tcp = TcpStream::connect(addr).await.unwrap();
    drop(tcp);
    let error = server
        .await
        .unwrap()
        .expect("a closed connection cannot complete our handshake");
    assert_eq!(alert(&error), None, "{error}");
}

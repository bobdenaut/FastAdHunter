use std::net::Ipv4Addr;
use std::sync::Arc;

use fah_certs::{CaParams, CertStore, MintingResolver};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, RootCertStore, ServerConfig};

const HOST: &str = "test.example";

fn store_with_ca() -> (tempfile::TempDir, Arc<CertStore>) {
    fah_certs::install_crypto_provider();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CertStore::open(dir.path()).unwrap());
    store.generate_ca(&CaParams::default()).unwrap();
    (dir, store)
}

fn client_trusting(roots: Vec<CertificateDer<'static>>) -> Arc<ClientConfig> {
    let mut store = RootCertStore::empty();
    for root in roots {
        store.add(root).unwrap();
    }
    Arc::new(
        ClientConfig::builder()
            .with_root_certificates(store)
            .with_no_client_auth(),
    )
}

async fn serve(resolver: Arc<MintingResolver>) -> std::net::SocketAddr {
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let _ = acceptor.accept(stream).await;
            });
        }
    });
    addr
}

async fn connect(
    addr: std::net::SocketAddr,
    client: Arc<ClientConfig>,
    name: ServerName<'static>,
) -> Result<(), std::io::Error> {
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    tokio_rustls::TlsConnector::from(client)
        .connect(name, stream)
        .await
        .map(|_| ())
}

#[tokio::test]
async fn a_client_trusting_only_the_exported_ca_completes_a_handshake_against_a_minted_leaf() {
    let (_dir, store) = store_with_ca();
    let exported = store.ca_public_der().unwrap();
    let addr = serve(Arc::new(MintingResolver::new(Arc::clone(&store), None))).await;
    store.prewarm(HOST).unwrap();

    let trusting = client_trusting(vec![CertificateDer::from(exported)]);
    connect(addr, trusting, ServerName::try_from(HOST).unwrap())
        .await
        .expect("a client trusting only the exported CA must verify a minted leaf");

    assert_eq!(store.leaf_cache_stats().minted_total, 1);

    let untrusting = client_trusting(Vec::new());
    connect(addr, untrusting, ServerName::try_from(HOST).unwrap())
        .await
        .expect_err("a client trusting nothing must reject the minted leaf");
}

#[tokio::test]
async fn a_hello_without_sni_gets_the_fallback_or_aborts_the_handshake() {
    let (_dir, store) = store_with_ca();
    let exported = store.ca_public_der().unwrap();
    let ip = ServerName::IpAddress(Ipv4Addr::LOCALHOST.into());

    let fail_closed = serve(Arc::new(MintingResolver::new(Arc::clone(&store), None))).await;
    connect(
        fail_closed,
        client_trusting(vec![CertificateDer::from(exported.clone())]),
        ip.clone(),
    )
    .await
    .expect_err("no SNI and no fallback must abort the handshake");

    let fallback = store.prewarm("127.0.0.1").unwrap();
    let serving = serve(Arc::new(MintingResolver::new(
        Arc::clone(&store),
        Some(fallback),
    )))
    .await;
    connect(
        serving,
        client_trusting(vec![CertificateDer::from(exported)]),
        ip,
    )
    .await
    .expect("a fallback key must serve a client that sends no SNI");
}

#[tokio::test]
async fn a_leaf_minted_after_regeneration_chains_to_the_new_authority_only() {
    let (_dir, store) = store_with_ca();
    let old_root = store.ca_public_der().unwrap();
    let addr = serve(Arc::new(MintingResolver::new(Arc::clone(&store), None))).await;

    store.prewarm(HOST).unwrap();
    connect(
        addr,
        client_trusting(vec![CertificateDer::from(old_root.clone())]),
        ServerName::try_from(HOST).unwrap(),
    )
    .await
    .expect("the leaf minted under the first authority verifies against it");

    store.generate_ca(&CaParams::default()).unwrap();
    assert_eq!(
        store.leaf_cache_stats().size,
        0,
        "regeneration must purge leaves signed by the archived authority"
    );
    store.prewarm(HOST).unwrap();
    let new_root = store.ca_public_der().unwrap();

    connect(
        addr,
        client_trusting(vec![CertificateDer::from(new_root)]),
        ServerName::try_from(HOST).unwrap(),
    )
    .await
    .expect("a client trusting the new root must verify the re-minted leaf");

    connect(
        addr,
        client_trusting(vec![CertificateDer::from(old_root)]),
        ServerName::try_from(HOST).unwrap(),
    )
    .await
    .expect_err("the archived root must no longer verify what the store serves");
}

#[tokio::test]
async fn an_unwarmed_host_is_refused_rather_than_minted_during_the_handshake() {
    let (_dir, store) = store_with_ca();
    let exported = store.ca_public_der().unwrap();
    let addr = serve(Arc::new(MintingResolver::new(Arc::clone(&store), None))).await;

    connect(
        addr,
        client_trusting(vec![CertificateDer::from(exported)]),
        ServerName::try_from(HOST).unwrap(),
    )
    .await
    .expect_err("an unwarmed host with no fallback must abort, not mint on the handshake");

    let stats = store.leaf_cache_stats();
    assert_eq!(
        stats.minted_total, 0,
        "no crypto may run on the handshake path"
    );
    assert_eq!(stats.unwarmed_misses, 1);
}

#[tokio::test]
async fn an_unwarmed_host_falls_back_when_a_fallback_is_set() {
    let (dir, store) = store_with_ca();
    fah_certs::load_or_generate(dir.path(), "127.0.0.1", None).unwrap();
    let fallback = store.api_certified_key().unwrap();
    let roots = fallback
        .cert
        .iter()
        .map(|der| der.clone().into_owned())
        .collect::<Vec<_>>();

    let addr = serve(Arc::new(MintingResolver::new(
        Arc::clone(&store),
        Some(fallback),
    )))
    .await;

    connect(
        addr,
        client_trusting(roots),
        ServerName::IpAddress(Ipv4Addr::LOCALHOST.into()),
    )
    .await
    .expect("the fallback serves a hello the cache cannot answer");

    assert_eq!(store.leaf_cache_stats().minted_total, 0);
}

#[tokio::test]
async fn the_api_pair_serves_a_no_sni_hello_as_the_resolver_fallback() {
    let (dir, store) = store_with_ca();
    fah_certs::load_or_generate(dir.path(), "127.0.0.1", None).unwrap();

    let fallback = store.api_certified_key().unwrap();
    let roots = fallback
        .cert
        .iter()
        .map(|der| der.clone().into_owned())
        .collect::<Vec<_>>();

    let addr = serve(Arc::new(MintingResolver::new(
        Arc::clone(&store),
        Some(fallback),
    )))
    .await;

    connect(
        addr,
        client_trusting(roots),
        ServerName::IpAddress(Ipv4Addr::LOCALHOST.into()),
    )
    .await
    .expect("p3-05's no-SNI fallback must be constructible from fah-certs alone");
}

#[test]
fn an_imported_authority_exports_no_private_material() {
    fah_certs::install_crypto_provider();
    let dir = tempfile::tempdir().unwrap();
    let store = CertStore::open(dir.path()).unwrap();
    store.generate_ca(&CaParams::default()).unwrap();

    let cert_pem = store.ca_public_pem().unwrap();
    let key_pem = std::fs::read_to_string(dir.path().join("ca-key.pem")).unwrap();

    let elsewhere = tempfile::tempdir().unwrap();
    let target = CertStore::open(elsewhere.path()).unwrap();
    let pair = fah_certs::validate_ca_pair(&format!("{cert_pem}{key_pem}"), &key_pem).unwrap();
    target.install_ca_pair(&pair).unwrap();

    let exported = target.ca_public_pem().unwrap();
    assert_eq!(exported.matches("BEGIN CERTIFICATE").count(), 1);
    assert!(!exported.contains("PRIVATE"));
    assert!(
        !std::fs::read_to_string(elsewhere.path().join("ca-cert.pem"))
            .unwrap()
            .contains("PRIVATE")
    );
}

#[test]
fn every_export_path_is_free_of_private_material() {
    let (_dir, store) = store_with_ca();

    let pem = store.ca_public_pem().unwrap();
    assert_eq!(pem.matches("BEGIN CERTIFICATE").count(), 1);
    assert!(!pem.contains("PRIVATE"));

    let der = store.ca_public_der().unwrap();
    assert!(
        x509_parser::parse_x509_certificate(&der).is_ok(),
        "the DER export must parse as a certificate"
    );
    assert!(!String::from_utf8_lossy(&der).contains("PRIVATE"));

    let rendered = format!("{:?}", store.status());
    assert!(!rendered.contains("PRIVATE"));
}

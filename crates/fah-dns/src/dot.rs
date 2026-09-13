use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use fah_certs::{CertStore, MintingResolver};
use rustls::server::Acceptor;
use rustls::sign::CertifiedKey;
use rustls::ServerConfig;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::time::{timeout, timeout_at, Instant};
use tokio_rustls::LazyConfigAcceptor;
use tracing::{debug, warn};

use crate::backoff::{RetryDecision, RetryPolicy};
use crate::pipeline::{Pipeline, Transport};
use crate::server::ListenerDied;
use crate::tcp;
use crate::upstream::Forwarder;

pub const DOT_MAX_CONNECTIONS: usize = 64;

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct DotTls {
    config: Arc<ServerConfig>,
    store: Arc<CertStore>,
}

impl DotTls {
    pub fn new(store: Arc<CertStore>, fallback: Arc<CertifiedKey>) -> Result<Self, rustls::Error> {
        let resolver = MintingResolver::new(Arc::clone(&store), Some(fallback));
        let config = ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(resolver));
        Ok(Self {
            config: Arc::new(config),
            store,
        })
    }
}

pub async fn run<F: Forwarder>(
    listener: TcpListener,
    tls: DotTls,
    pipeline: Arc<Pipeline<F>>,
) -> ListenerDied {
    run_with(listener, tls, pipeline, HANDSHAKE_TIMEOUT).await
}

pub(crate) async fn run_with<F: Forwarder>(
    listener: TcpListener,
    tls: DotTls,
    pipeline: Arc<Pipeline<F>>,
    handshake_timeout: Duration,
) -> ListenerDied {
    let slots = Arc::new(Semaphore::new(DOT_MAX_CONNECTIONS));
    let mut policy = RetryPolicy::new();
    loop {
        let Ok(slot) = Arc::clone(&slots).acquire_owned().await else {
            return ListenerDied {
                last_error: io::Error::other("the DoT connection semaphore closed"),
            };
        };
        let (stream, client) = match listener.accept().await {
            Ok(pair) => {
                policy.on_success();
                pair
            }
            Err(err) => match policy.on_error() {
                RetryDecision::Sleep(delay) => {
                    warn!(
                        error = %err,
                        retry_in_ms = delay.as_millis(),
                        "DoT listener accept failed; retrying"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                RetryDecision::Fatal => return ListenerDied { last_error: err },
            },
        };
        if let Err(err) = stream.set_nodelay(true) {
            debug!(error = %err, client = %client, "TCP_NODELAY not set on a DoT connection");
        }
        let pipeline = Arc::clone(&pipeline);
        let tls = tls.clone();
        tokio::spawn(async move {
            let _slot = slot;
            let served = serve_connection(stream, client, tls, &pipeline, handshake_timeout).await;
            tcp::report_connection_end(served, client, "DoT");
        });
    }
}

async fn serve_connection<F: Forwarder>(
    stream: TcpStream,
    client: SocketAddr,
    tls: DotTls,
    pipeline: &Pipeline<F>,
    handshake_timeout: Duration,
) -> io::Result<()> {
    let deadline = Instant::now() + handshake_timeout;
    let start = match timeout_at(
        deadline,
        LazyConfigAcceptor::new(Acceptor::default(), stream),
    )
    .await
    {
        Ok(Ok(start)) => start,
        Ok(Err(err)) => {
            debug!(client = %client, error = %err, "DoT ClientHello rejected");
            return Ok(());
        }
        Err(_elapsed) => {
            debug!(client = %client, "DoT ClientHello not received in time");
            return Ok(());
        }
    };
    let sni = match start.client_hello().server_name() {
        Some(host) if tls.store.has_ca() => Some(host.to_owned()),
        _ => None,
    };
    #[cfg(feature = "diag-timing")]
    let mut diag = DiagTiming::at_sni();
    if let Some(host) = sni {
        #[cfg(not(feature = "diag-timing"))]
        prewarm(&tls.store, host).await;
        #[cfg(feature = "diag-timing")]
        prewarm(&tls.store, host, &mut diag).await;
    }
    let mut stream = match timeout_at(deadline, start.into_stream(tls.config)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(err)) => {
            debug!(client = %client, error = %err, "DoT handshake failed");
            return Ok(());
        }
        Err(_elapsed) => {
            debug!(client = %client, "DoT handshake timed out");
            return Ok(());
        }
    };
    #[cfg(feature = "diag-timing")]
    diag.emit(client);
    let served =
        tcp::handle_connection(&mut stream, pipeline, client.ip(), Transport::Dot, None).await;
    let _ = timeout(tcp::TCP_IDLE_TIMEOUT, stream.shutdown()).await;
    served
}

async fn prewarm(
    store: &Arc<CertStore>,
    host: String,
    #[cfg(feature = "diag-timing")] diag: &mut DiagTiming,
) {
    let store = Arc::clone(store);
    let logged = host.clone();
    #[cfg(feature = "diag-timing")]
    let joined = {
        diag.dispatching();
        tokio::task::spawn_blocking(move || {
            let entered = std::time::Instant::now();
            let minted = store.prewarm(&host);
            (minted, entered, std::time::Instant::now())
        })
        .await
        .map(|(minted, entered, prewarmed)| {
            diag.minted(entered, prewarmed);
            minted
        })
    };
    #[cfg(not(feature = "diag-timing"))]
    let joined = tokio::task::spawn_blocking(move || store.prewarm(&host)).await;
    match joined {
        Ok(Ok(_)) => {}
        Ok(Err(err)) => {
            debug!(host = %logged, error = %err, "DoT leaf not minted; serving the fallback certificate")
        }
        Err(err) => warn!(host = %logged, error = %err, "DoT leaf pre-warm task failed"),
    }
}

#[cfg(feature = "diag-timing")]
struct DiagTiming {
    sni_parsed: std::time::Instant,
    dispatched: std::time::Instant,
    entered: std::time::Instant,
    prewarmed: std::time::Instant,
}

#[cfg(feature = "diag-timing")]
impl DiagTiming {
    fn at_sni() -> Self {
        let now = std::time::Instant::now();
        Self {
            sni_parsed: now,
            dispatched: now,
            entered: now,
            prewarmed: now,
        }
    }

    fn dispatching(&mut self) {
        self.dispatched = std::time::Instant::now();
        self.entered = self.dispatched;
        self.prewarmed = self.dispatched;
    }

    fn minted(&mut self, entered: std::time::Instant, prewarmed: std::time::Instant) {
        self.entered = entered;
        self.prewarmed = prewarmed;
    }

    fn emit(&self, client: SocketAddr) {
        let handshaken = std::time::Instant::now();
        tracing::info!(
            client = %client,
            sni_to_dispatch_us = micros_between(self.sni_parsed, self.dispatched),
            dispatch_wait_us = micros_between(self.dispatched, self.entered),
            prewarm_us = micros_between(self.entered, self.prewarmed),
            handshake_after_prewarm_us = micros_between(self.prewarmed, handshaken),
            "DoT first-sight timing"
        );
    }
}

#[cfg(feature = "diag-timing")]
fn micros_between(from: std::time::Instant, to: std::time::Instant) -> u64 {
    u64::try_from(to.duration_since(from).as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use fah_certs::CaParams;
    use hickory_proto::op::{Message, Query as WireQuery};
    use hickory_proto::rr::{Name, RecordType};
    use rustls::pki_types::{
        CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime,
    };
    use rustls::{ClientConfig, RootCertStore};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::time::timeout;
    use tokio_rustls::TlsConnector;

    use super::*;
    use crate::testkit;

    const HOST: &str = "dns.fah.test";

    fn store_with_ca() -> (tempfile::TempDir, Arc<CertStore>) {
        fah_certs::install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CertStore::open(dir.path()).unwrap());
        store.generate_ca(&CaParams::default()).unwrap();
        (dir, store)
    }

    fn self_signed_fallback() -> (Arc<CertifiedKey>, CertificateDer<'static>) {
        let signed = rcgen::generate_simple_self_signed(vec!["fallback.test".to_string()]).unwrap();
        let cert = signed.cert.der().clone();
        let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(signed.signing_key.serialize_der()));
        let signing_key = rustls::crypto::aws_lc_rs::sign::any_supported_type(&key).unwrap();
        (
            Arc::new(CertifiedKey::new(vec![cert.clone()], signing_key)),
            cert,
        )
    }

    fn client_trusting(roots: &[CertificateDer<'static>]) -> Arc<ClientConfig> {
        let mut root_store = RootCertStore::empty();
        for root in roots {
            root_store.add(root.clone()).unwrap();
        }
        Arc::new(
            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth(),
        )
    }

    #[derive(Debug)]
    struct AcceptAny;

    impl rustls::client::danger::ServerCertVerifier for AcceptAny {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::aws_lc_rs::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    fn client_accepting_any() -> Arc<ClientConfig> {
        Arc::new(
            ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(AcceptAny))
                .with_no_client_auth(),
        )
    }

    struct Listener {
        addr: SocketAddr,
        task: tokio::task::JoinHandle<ListenerDied>,
        _data_dir: tempfile::TempDir,
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn listen(tls: DotTls, handshake_timeout: Duration) -> Listener {
        let (pipeline, data_dir) = testkit::pipeline();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(run_with(listener, tls, pipeline, handshake_timeout));
        Listener {
            addr,
            task,
            _data_dir: data_dir,
        }
    }

    async fn connect(
        addr: SocketAddr,
        client: Arc<ClientConfig>,
        sni: &str,
    ) -> io::Result<tokio_rustls::client::TlsStream<TcpStream>> {
        let stream = TcpStream::connect(addr).await?;
        let name = ServerName::try_from(sni.to_string()).unwrap();
        TlsConnector::from(client).connect(name, stream).await
    }

    fn a_query() -> Vec<u8> {
        let mut message = Message::query();
        message.add_query(WireQuery::query(
            Name::from_ascii("example.com.").unwrap(),
            RecordType::A,
        ));
        message.to_vec().unwrap()
    }

    async fn exchange<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
        stream: &mut S,
        request: &[u8],
    ) -> Vec<u8> {
        let len = u16::try_from(request.len()).unwrap().to_be_bytes();
        stream.write_all(&len).await.unwrap();
        stream.write_all(request).await.unwrap();
        let mut len_buf = [0u8; 2];
        stream.read_exact(&mut len_buf).await.unwrap();
        let mut reply = vec![0u8; u16::from_be_bytes(len_buf) as usize];
        stream.read_exact(&mut reply).await.unwrap();
        reply
    }

    fn peer_leaf_is_issued_by(
        stream: &tokio_rustls::client::TlsStream<TcpStream>,
        ca: &CertificateDer<'static>,
    ) -> bool {
        let (_, session) = stream.get_ref();
        let leaf = &session.peer_certificates().unwrap()[0];
        let (_, leaf) = x509_parser::parse_x509_certificate(leaf).unwrap();
        let (_, ca) = x509_parser::parse_x509_certificate(ca).unwrap();
        leaf.issuer() == ca.subject() && leaf.subject() != ca.subject()
    }

    #[tokio::test]
    async fn a_hello_with_sni_is_served_a_ca_minted_leaf_on_its_first_handshake() {
        let (_dir, store) = store_with_ca();
        let (fallback, _) = self_signed_fallback();
        let ca_der = CertificateDer::from(store.ca_public_der().unwrap());
        let tls = DotTls::new(Arc::clone(&store), fallback).unwrap();
        let listener = listen(tls, HANDSHAKE_TIMEOUT).await;

        assert!(store.cached_leaf(HOST).is_none());
        let client = client_trusting(std::slice::from_ref(&ca_der));
        let mut stream = connect(listener.addr, client, HOST).await.unwrap();
        assert!(peer_leaf_is_issued_by(&stream, &ca_der));
        assert!(store.cached_leaf(HOST).is_some());

        let request = a_query();
        let reply = exchange(&mut stream, &request).await;
        assert_eq!(
            Message::from_vec(&reply).unwrap().metadata.id,
            Message::from_vec(&request).unwrap().metadata.id
        );
    }

    #[tokio::test]
    async fn a_hello_without_sni_or_without_a_ca_is_served_the_fallback() {
        let (_dir, store) = store_with_ca();
        let (fallback, fallback_der) = self_signed_fallback();
        let tls = DotTls::new(store, fallback).unwrap();
        let listener = listen(tls, HANDSHAKE_TIMEOUT).await;

        let stream = connect(
            listener.addr,
            client_accepting_any(),
            &Ipv4Addr::LOCALHOST.to_string(),
        )
        .await
        .unwrap();
        let (_, session) = stream.get_ref();
        assert_eq!(session.peer_certificates().unwrap()[0], fallback_der);

        let dir = tempfile::tempdir().unwrap();
        let no_ca = Arc::new(CertStore::open(dir.path()).unwrap());
        let (fallback, fallback_der) = self_signed_fallback();
        let listener = listen(DotTls::new(no_ca, fallback).unwrap(), HANDSHAKE_TIMEOUT).await;
        let stream = connect(listener.addr, client_accepting_any(), HOST)
            .await
            .unwrap();
        let (_, session) = stream.get_ref();
        assert_eq!(session.peer_certificates().unwrap()[0], fallback_der);
    }

    #[tokio::test]
    async fn an_evicted_leaf_is_re_minted_on_the_next_handshake_never_the_fallback() {
        let (_dir, store) = store_with_ca();
        let (fallback, fallback_der) = self_signed_fallback();
        let ca_der = CertificateDer::from(store.ca_public_der().unwrap());
        let tls = DotTls::new(Arc::clone(&store), fallback).unwrap();
        let listener = listen(tls, HANDSHAKE_TIMEOUT).await;

        store.prewarm(HOST).unwrap();
        for index in 0..fah_certs::LEAF_CACHE_CAPACITY {
            store.prewarm(&format!("host{index}.browsed.test")).unwrap();
        }
        assert!(
            store.cached_leaf(HOST).is_none(),
            "capacity many other hosts must evict the DoT hostname"
        );

        let client = client_trusting(std::slice::from_ref(&ca_der));
        let stream = connect(listener.addr, client, HOST).await.unwrap();
        let (_, session) = stream.get_ref();
        assert_ne!(session.peer_certificates().unwrap()[0], fallback_der);
        assert!(peer_leaf_is_issued_by(&stream, &ca_der));
        assert!(store.cached_leaf(HOST).is_some());
    }

    #[tokio::test]
    async fn a_finished_connection_is_closed_with_close_notify_not_a_bare_fin() {
        let (_dir, store) = store_with_ca();
        let (fallback, _) = self_signed_fallback();
        let listener = listen(DotTls::new(store, fallback).unwrap(), HANDSHAKE_TIMEOUT).await;

        let mut stream = connect(listener.addr, client_accepting_any(), HOST)
            .await
            .unwrap();
        stream.write_all(&[0x00, 0x01, 0x00]).await.unwrap();
        let mut buf = [0u8; 16];
        let read = timeout(Duration::from_secs(5), stream.read(&mut buf))
            .await
            .expect("the server must close a connection it will not serve");
        assert!(
            matches!(read, Ok(0)),
            "a TLS close must arrive as close_notify (clean EOF), got {read:?}"
        );
    }

    #[tokio::test]
    async fn plaintext_dns_on_the_dot_port_gets_no_answer() {
        let (_dir, store) = store_with_ca();
        let (fallback, _) = self_signed_fallback();
        let listener = listen(
            DotTls::new(store, fallback).unwrap(),
            Duration::from_millis(500),
        )
        .await;

        let mut stream = TcpStream::connect(listener.addr).await.unwrap();
        let request = a_query();
        let len = u16::try_from(request.len()).unwrap().to_be_bytes();
        stream.write_all(&len).await.unwrap();
        stream.write_all(&request).await.unwrap();
        let mut buf = [0u8; 512];
        let read = timeout(Duration::from_secs(5), stream.read(&mut buf))
            .await
            .expect("the server must close a plaintext connection");
        if let Ok(len) = read {
            assert!(
                len == 0 || buf[0] == 0x15,
                "only a TLS alert or a close may come back, got {:?}",
                &buf[..len]
            );
        }
    }

    #[tokio::test]
    async fn a_silent_connection_is_dropped_after_the_handshake_timeout() {
        let (_dir, store) = store_with_ca();
        let (fallback, _) = self_signed_fallback();
        let listener = listen(
            DotTls::new(store, fallback).unwrap(),
            Duration::from_millis(300),
        )
        .await;

        let mut stream = TcpStream::connect(listener.addr).await.unwrap();
        let mut buf = [0u8; 1];
        let started = std::time::Instant::now();
        let read = timeout(Duration::from_secs(5), stream.read(&mut buf))
            .await
            .expect("the server must close a silent connection");
        assert!(matches!(read, Ok(0) | Err(_)), "got {read:?}");
        assert!(started.elapsed() >= Duration::from_millis(250));
    }

    #[tokio::test]
    async fn the_connection_bound_queues_the_next_client_until_a_slot_frees() {
        let (_dir, store) = store_with_ca();
        let (fallback, _) = self_signed_fallback();
        let listener = listen(
            DotTls::new(store, fallback).unwrap(),
            Duration::from_secs(30),
        )
        .await;
        let client = client_accepting_any();

        let mut held = Vec::with_capacity(DOT_MAX_CONNECTIONS);
        for _ in 0..DOT_MAX_CONNECTIONS {
            held.push(
                connect(listener.addr, Arc::clone(&client), HOST)
                    .await
                    .unwrap(),
            );
        }

        let queued = timeout(
            Duration::from_millis(500),
            connect(listener.addr, Arc::clone(&client), HOST),
        )
        .await;
        assert!(
            queued.is_err(),
            "the {}th connection must wait, not be served",
            DOT_MAX_CONNECTIONS + 1
        );

        drop(held.pop());
        let served = timeout(
            Duration::from_secs(10),
            connect(listener.addr, client, HOST),
        )
        .await
        .expect("a freed slot must admit the next client")
        .unwrap();
        drop(served);
    }
}

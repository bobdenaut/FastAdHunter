use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use fah_certs::{CertStore, MintingResolver};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

const ALPN_H2: &[u8] = b"h2";
const ALPN_HTTP11: &[u8] = b"http/1.1";

fn alpn_both() -> Vec<Vec<u8>> {
    vec![ALPN_H2.to_vec(), ALPN_HTTP11.to_vec()]
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

pub fn server_config(store: Arc<CertStore>) -> Result<Arc<ServerConfig>, rustls::Error> {
    let mut config = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(MintingResolver::new(store, None)));
    config.alpn_protocols = alpn_both();
    Ok(Arc::new(config))
}

pub fn client_config() -> Result<Arc<ClientConfig>, rustls::Error> {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    client_config_with_roots(roots)
}

pub fn client_config_with_roots(roots: RootCertStore) -> Result<Arc<ClientConfig>, rustls::Error> {
    let mut config = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = alpn_both();
    Ok(Arc::new(config))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Alpn {
    H2,
    Http11,
}

pub(crate) fn negotiated(tls: &TlsStream<TcpStream>) -> Alpn {
    match tls.get_ref().1.alpn_protocol() {
        Some(ALPN_H2) => Alpn::H2,
        _ => Alpn::Http11,
    }
}

pub(crate) async fn connect_verified_upstream(
    config: &Arc<ClientConfig>,
    sni: &str,
    approved: SocketAddr,
    timeout: Duration,
) -> io::Result<TlsStream<TcpStream>> {
    let name = ServerName::try_from(sni.to_string())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
    let connect = async {
        let tcp = TcpStream::connect(approved).await?;
        tcp.set_nodelay(true)?;
        TlsConnector::from(Arc::clone(config))
            .connect(name, tcp)
            .await
    };
    tokio::time::timeout(timeout, connect)
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

pub(crate) fn certificate_error(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::InvalidData
        && err
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<rustls::Error>())
            .is_some_and(|inner| matches!(inner, rustls::Error::InvalidCertificate(_)))
}

pub(crate) fn client_alert(err: &io::Error) -> Option<rustls::AlertDescription> {
    if err.kind() != io::ErrorKind::InvalidData {
        return None;
    }
    match err
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        Some(rustls::Error::AlertReceived(description)) => Some(*description),
        _ => None,
    }
}

pub(crate) struct RewindStream<S> {
    buffered: Vec<u8>,
    at: usize,
    inner: S,
}

impl<S> RewindStream<S> {
    pub(crate) fn new(buffered: Vec<u8>, inner: S) -> Self {
        Self {
            buffered,
            at: 0,
            inner,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for RewindStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.at < self.buffered.len() {
            let take = (self.buffered.len() - self.at).min(buf.remaining());
            let at = self.at;
            buf.put_slice(&self.buffered[at..at + take]);
            self.at += take;
            if self.at == self.buffered.len() {
                self.buffered = Vec::new();
                self.at = 0;
            }
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for RewindStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, data)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn a_rewound_stream_yields_the_buffer_first_then_the_socket_in_order() {
        let (ours, mut theirs) = tokio::io::duplex(64);
        theirs.write_all(b"socket bytes").await.unwrap();
        let mut rewound = RewindStream::new(b"hello bytes ".to_vec(), ours);

        let mut first = [0u8; 5];
        rewound.read_exact(&mut first).await.unwrap();
        assert_eq!(&first, b"hello");

        let mut rest = [0u8; 7 + 12];
        rewound.read_exact(&mut rest).await.unwrap();
        assert_eq!(&rest, b" bytes socket bytes");
        assert!(
            rewound.buffered.is_empty(),
            "the replay buffer is released once drained"
        );

        rewound.write_all(b"reply").await.unwrap();
        let mut reply = [0u8; 5];
        theirs.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"reply");
    }

    #[tokio::test]
    async fn a_read_larger_than_the_buffer_does_not_block_on_the_socket() {
        let (ours, _theirs) = tokio::io::duplex(64);
        let mut rewound = RewindStream::new(b"abc".to_vec(), ours);
        let mut buf = [0u8; 16];
        let read = rewound.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..read], b"abc");
    }

    #[test]
    fn only_an_invalid_certificate_is_a_certificate_error() {
        let wrap = |inner: rustls::Error| io::Error::new(io::ErrorKind::InvalidData, inner);
        assert!(certificate_error(&wrap(rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnknownIssuer
        ))));
        assert!(certificate_error(&wrap(rustls::Error::InvalidCertificate(
            rustls::CertificateError::NotValidForName
        ))));
        assert!(!certificate_error(&wrap(
            rustls::Error::NoApplicationProtocol
        )));
        assert!(!certificate_error(&wrap(rustls::Error::AlertReceived(
            rustls::AlertDescription::HandshakeFailure
        ))));
        assert!(!certificate_error(&wrap(rustls::Error::AlertReceived(
            rustls::AlertDescription::UnrecognisedName
        ))));
        assert!(!certificate_error(&io::Error::from(
            io::ErrorKind::ConnectionRefused
        )));
        assert!(!certificate_error(&io::Error::from(
            io::ErrorKind::TimedOut
        )));
        assert!(!certificate_error(&io::Error::new(
            io::ErrorKind::InvalidData,
            "not a rustls error"
        )));
    }

    #[test]
    fn client_alert_matches_only_an_alert_received() {
        let wrap = |inner: rustls::Error| io::Error::new(io::ErrorKind::InvalidData, inner);
        assert_eq!(
            client_alert(&wrap(rustls::Error::AlertReceived(
                rustls::AlertDescription::AccessDenied
            ))),
            Some(rustls::AlertDescription::AccessDenied)
        );
        assert_eq!(
            client_alert(&wrap(rustls::Error::AlertReceived(
                rustls::AlertDescription::UnknownCA
            ))),
            Some(rustls::AlertDescription::UnknownCA)
        );
        assert_eq!(
            client_alert(&wrap(rustls::Error::InvalidCertificate(
                rustls::CertificateError::UnknownIssuer
            ))),
            None
        );
        assert_eq!(
            client_alert(&wrap(rustls::Error::NoApplicationProtocol)),
            None
        );
        assert_eq!(
            client_alert(&io::Error::from(io::ErrorKind::UnexpectedEof)),
            None
        );
        assert_eq!(
            client_alert(&io::Error::new(
                io::ErrorKind::InvalidData,
                "not a rustls error"
            )),
            None
        );
        assert_eq!(
            client_alert(&io::Error::new(
                io::ErrorKind::ConnectionReset,
                rustls::Error::AlertReceived(rustls::AlertDescription::BadCertificate)
            )),
            None
        );
    }

    #[tokio::test]
    async fn a_name_rustls_rejects_never_reaches_a_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let config = client_config().unwrap();
        let err = connect_verified_upstream(&config, "origin.123", addr, Duration::from_secs(1))
            .await
            .expect_err("an all-numeric last label is not a DNS name for rustls");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!certificate_error(&err));
        let knocked = tokio::time::timeout(Duration::from_millis(100), listener.accept()).await;
        assert!(
            knocked.is_err(),
            "a name that cannot be verified must not open a connection"
        );
    }

    #[test]
    fn both_configs_offer_h2_then_http11() {
        let client = client_config().unwrap();
        assert_eq!(client.alpn_protocols, alpn_both());
        assert_eq!(alpn_both()[0], b"h2");
    }
}

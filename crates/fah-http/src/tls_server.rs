use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use fah_common::listen::{bind_error, bind_tcp, listen_addr};
use fah_config::HttpsConfig;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

use crate::connections::ConnectionGauge;
use crate::https::TlsProxy;
use crate::server::{accept_loop, Dispatch, Lane, Server};

const PORT_SETTING: &str = "[https.listen] port, or FAH__HTTPS__LISTEN__PORT";

#[derive(Debug)]
pub struct TlsServer {
    local_addr: SocketAddr,
    listener: Option<TcpListener>,
    permits: Arc<Semaphore>,
    connections: Arc<ConnectionGauge>,
    handle: Option<JoinHandle<()>>,
}

impl TlsServer {
    pub async fn bind(config: &HttpsConfig) -> io::Result<Self> {
        let addr = listen_addr(&config.listen.address, config.listen.port, "https.listen")?;
        let listener = bind_tcp(addr)
            .await
            .map_err(|err| bind_error("TCP", addr, err, PORT_SETTING))?;
        let local_addr = listener.local_addr()?;

        Ok(Self {
            local_addr,
            listener: Some(listener),
            permits: Arc::new(Semaphore::new(config.max_connections)),
            connections: Arc::new(ConnectionGauge::default()),
            handle: None,
        })
    }

    pub fn serve(&mut self, proxy: Arc<TlsProxy>) {
        self.start(Dispatch::SharedTls(proxy));
    }

    pub fn serve_domains(&mut self, proxy: Arc<TlsProxy>, http: &Server) -> io::Result<()> {
        let rotation = http.rotation();
        if rotation.is_empty() {
            return Err(io::Error::other(
                "the HTTP listener runs no allocation domain for HTTPS to feed",
            ));
        }
        self.start(Dispatch::Domains {
            rotation,
            lane: Lane::Https(proxy),
        });
        Ok(())
    }

    fn start(&mut self, dispatch: Dispatch) {
        let Some(listener) = self.listener.take() else {
            return;
        };
        let permits = Arc::clone(&self.permits);
        let connections = Arc::clone(&self.connections);
        self.handle = Some(tokio::spawn(accept_loop(
            listener,
            permits,
            connections,
            dispatch,
        )));
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn connections(&self) -> Arc<ConnectionGauge> {
        Arc::clone(&self.connections)
    }

    pub fn shutdown(&self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use fah_common::egress::DestinationPolicy;
    use fah_common::resolve::{HostResolver, Resolving};
    use fah_config::NoSni;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpStream;

    use super::*;

    fn config(port: u16) -> HttpsConfig {
        HttpsConfig {
            listen: fah_config::HttpsListenConfig {
                address: "127.0.0.1".to_string(),
                port,
            },
            ..HttpsConfig::default()
        }
    }

    struct NoResolver;

    impl HostResolver for NoResolver {
        fn resolve(&self, _host: String) -> Resolving {
            Box::pin(async {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no resolver in this test",
                ))
            })
        }
    }

    fn proxy_with(hello_timeout: Duration) -> Arc<TlsProxy> {
        Arc::new(TlsProxy::new(
            Arc::new(NoResolver),
            DestinationPolicy::new(443, Vec::new()),
            443,
            hello_timeout,
            Duration::from_secs(1),
            NoSni::Pass,
        ))
    }

    async fn ip_literal_sni_outcome(allow: bool) -> crate::ProxyStats {
        let mut server = TlsServer::bind(&config(0)).await.unwrap();
        let addr = server.local_addr();
        let proxy = Arc::new(
            TlsProxy::new(
                Arc::new(NoResolver),
                DestinationPolicy::new(443, Vec::new()),
                443,
                Duration::from_secs(5),
                Duration::from_secs(1),
                NoSni::Pass,
            )
            .with_ip_literal_hosts(allow),
        );
        let counters = proxy.counters();
        server.serve(proxy);

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(&crate::sni::tests::hello(Some("1.2.3.4")))
            .await
            .unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
            .await
            .expect("an IP-literal SNI must be answered by a close, not held")
            .unwrap();
        assert!(response.is_empty());
        server.shutdown();
        counters.snapshot()
    }

    #[tokio::test]
    async fn an_ip_literal_sni_is_refused_before_resolution_unless_allowed() {
        let refused = ip_literal_sni_outcome(false).await;
        assert_eq!(refused.refused_claim, 1);
        assert_eq!(refused.resolve_failures, 0);

        let allowed = ip_literal_sni_outcome(true).await;
        assert_eq!(allowed.refused_claim, 0);
        assert_eq!(allowed.resolve_failures, 1);
    }

    #[tokio::test]
    async fn bind_reports_the_port_the_os_actually_gave() {
        let server = TlsServer::bind(&config(0)).await.unwrap();
        assert_ne!(server.local_addr().port(), 0);
    }

    #[tokio::test]
    async fn an_invalid_listen_address_is_rejected_by_section_name() {
        let config = HttpsConfig {
            listen: fah_config::HttpsListenConfig {
                address: "not-an-ip".to_string(),
                port: 0,
            },
            ..HttpsConfig::default()
        };
        let err = TlsServer::bind(&config).await.unwrap_err();
        assert!(err.to_string().contains("https.listen"), "{err}");
    }

    #[tokio::test]
    async fn a_client_that_never_sends_a_hello_is_cut_off() {
        let mut server = TlsServer::bind(&config(0)).await.unwrap();
        let addr = server.local_addr();
        let proxy = proxy_with(Duration::from_millis(150));
        let counters = proxy.counters();
        server.serve(proxy);

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(&[0x16, 0x03, 0x01]).await.unwrap();

        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
            .await
            .expect("the hello deadline must close a stalled handshake")
            .unwrap();
        assert!(response.is_empty());
        let counters = counters.snapshot();
        assert_eq!(counters.hello_timeouts, 1);
        assert_eq!(counters.non_tls, 0);
        server.shutdown();
    }

    #[tokio::test]
    async fn the_gauge_counts_open_connections_and_keeps_the_interval_peak() {
        let mut server = TlsServer::bind(&config(0)).await.unwrap();
        let addr = server.local_addr();
        let connections = server.connections();
        server.serve(proxy_with(Duration::from_secs(30)));

        let mut first = TcpStream::connect(addr).await.unwrap();
        first.write_all(&[0x16, 0x03, 0x01]).await.unwrap();
        let mut second = TcpStream::connect(addr).await.unwrap();
        second.write_all(&[0x16, 0x03, 0x01]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(connections.open(), 2);

        drop(first);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(connections.open(), 1);
        assert_eq!(connections.take_peak(), 2);
        assert_eq!(connections.take_peak(), 1);
        drop(second);
        server.shutdown();
    }

    #[tokio::test]
    async fn max_connections_actually_blocks_the_second_connection() {
        let config = HttpsConfig {
            max_connections: 1,
            ..config(0)
        };
        let mut server = TlsServer::bind(&config).await.unwrap();
        assert_eq!(server.permits.available_permits(), 1);
        let addr = server.local_addr();
        server.serve(proxy_with(Duration::from_secs(30)));

        let mut holder = TcpStream::connect(addr).await.unwrap();
        holder.write_all(&[0x16, 0x03, 0x01]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            server.permits.available_permits(),
            0,
            "the in-flight connection must hold the permit"
        );

        let mut queued = TcpStream::connect(addr).await.unwrap();
        queued.write_all(b"not tls at all").await.unwrap();
        let mut buf = [0u8; 1];
        let serviced =
            tokio::time::timeout(Duration::from_millis(300), queued.read(&mut buf)).await;
        assert!(
            serviced.is_err(),
            "the ceiling must hold the second connection in the kernel queue"
        );

        drop(holder);
        let read = tokio::time::timeout(Duration::from_secs(5), queued.read(&mut buf))
            .await
            .expect("a released permit must let the queued connection through");
        assert_eq!(read.unwrap(), 0, "non-TLS bytes are closed, not answered");
        server.shutdown();
    }
}

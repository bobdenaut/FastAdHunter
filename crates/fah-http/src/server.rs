//! Binds `[http.listen]` and runs the accept loop.
//!
//! Mirrors `fah_dns::Server`'s bind/serve split on purpose: binding may need
//! privilege, serving must not have it (ADR-0004). The shared dual-stack bind
//! lives in `fah_common::listen` so the two engines cannot drift on
//! `IPV6_V6ONLY` — an HTTP listener that quietly refused IPv6 while DNS served
//! it would be invisible until a v6-only client failed.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use fah_common::listen::{bind_error, bind_tcp, listen_addr};
use fah_config::HttpConfig;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

use crate::connections::ConnectionGauge;
use crate::proxy::Proxy;

/// Named in bind failures so the operator is sent to the right setting.
const PORT_SETTING: &str = "[http.listen] port, or FAH__HTTP__LISTEN__PORT";

/// Owns the accept loop. Dropping this does not stop it — call
/// [`Server::shutdown`], matching `fah_dns::Server`.
#[derive(Debug)]
pub struct Server {
    local_addr: SocketAddr,
    /// Bound, not yet accepting — taken by [`Server::serve`].
    listener: Option<TcpListener>,
    /// Concurrency ceiling from `[http] max_connections`. A permit is held for
    /// the life of a connection, so memory is bounded by configuration rather
    /// than by how many clients show up (CLAUDE.md hard rule 4).
    permits: Arc<Semaphore>,
    connections: Arc<ConnectionGauge>,
    handle: Option<JoinHandle<()>>,
}

impl Server {
    /// Binds the listener **without** accepting anything yet.
    ///
    /// Split from [`Server::serve`] for the same reason as DNS: the default
    /// port is 8080 and needs no privilege, but the split keeps one shape
    /// across engines and leaves port 80 usable where a runtime does allow it.
    pub async fn bind(config: &HttpConfig) -> io::Result<Self> {
        let addr = listen_addr(&config.listen.address, config.listen.port, "http.listen")?;
        let listener = bind_tcp(addr)
            .await
            .map_err(|err| bind_error("TCP", addr, err, PORT_SETTING))?;
        // `port == 0` (tests only) asks the OS for an ephemeral port, so read
        // back what was actually bound rather than assuming `addr`.
        let local_addr = listener.local_addr()?;

        Ok(Self {
            local_addr,
            listener: Some(listener),
            permits: Arc::new(Semaphore::new(config.max_connections)),
            connections: Arc::new(ConnectionGauge::default()),
            handle: None,
        })
    }

    /// Spawns the accept loop. Call after any privilege drop; a second call
    /// does nothing, since the listener has already been handed over.
    pub fn serve(&mut self, proxy: Arc<Proxy>) {
        let Some(listener) = self.listener.take() else {
            return;
        };
        let permits = Arc::clone(&self.permits);
        let connections = Arc::clone(&self.connections);
        self.handle = Some(tokio::spawn(accept_loop(
            listener,
            permits,
            connections,
            proxy,
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

/// Accepts connections and hands each to the proxy.
///
/// The permit is acquired *before* `accept`, not after: taking the connection
/// off the queue first and then waiting would let the backlog convert into
/// in-process state, which is the bound `max_connections` exists to hold. At
/// the ceiling the loop simply stops accepting and the kernel queues, which is
/// the behaviour a client's own timeout is designed for.
///
/// Since p2-02 the permit is held for the whole transfer rather than released
/// immediately, so the ceiling now genuinely binds — the gap p2-01 documented.
async fn accept_loop(
    listener: TcpListener,
    permits: Arc<Semaphore>,
    connections: Arc<ConnectionGauge>,
    proxy: Arc<Proxy>,
) {
    loop {
        let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
            // Semaphore closed — only on shutdown.
            return;
        };
        match listener.accept().await {
            Ok((stream, peer)) => {
                let proxy = Arc::clone(&proxy);
                let open = connections.enter();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _open = open;
                    // Proxied writes are small and latency-visible; Nagle would
                    // hold a request head waiting for more to send.
                    if let Err(err) = stream.set_nodelay(true) {
                        tracing::debug!(%peer, error = %err, "could not set TCP_NODELAY");
                    }
                    proxy.serve_connection(stream, peer).await;
                });
            }
            Err(err) => {
                // Per-connection failures (a peer that vanished between the
                // SYN and the accept, a momentary fd exhaustion) must not kill
                // the listener — the DNS TCP loop takes the same line.
                tracing::debug!(error = %err, "HTTP accept failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use fah_common::egress::DestinationPolicy;
    use fah_common::resolve::{HostResolver, Resolving};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpStream;

    use super::*;

    fn config(port: u16) -> HttpConfig {
        HttpConfig {
            listen: fah_config::HttpListenConfig {
                address: "127.0.0.1".to_string(),
                port,
            },
            ..HttpConfig::default()
        }
    }

    /// Resolves nothing: these tests exercise the listener and the connection
    /// ceiling, never a forwarded request.
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

    fn proxy_with(header_timeout: Duration) -> Arc<Proxy> {
        Arc::new(Proxy::new(
            Arc::new(NoResolver),
            DestinationPolicy::new(80, Vec::new()),
            80,
            header_timeout,
            Duration::from_secs(1),
            1,
            false,
        ))
    }

    /// Long enough that the header timeout never fires incidentally — tests
    /// that want it use [`proxy_with`] and say so.
    fn proxy() -> Arc<Proxy> {
        proxy_with(Duration::from_secs(5))
    }

    #[tokio::test]
    async fn bind_reports_the_port_the_os_actually_gave() {
        let server = Server::bind(&config(0)).await.unwrap();
        assert_ne!(
            server.local_addr().port(),
            0,
            "an ephemeral bind must report the resolved port, not 0"
        );
    }

    /// Binding must not start answering — ADR-0004's whole point is that the
    /// privileged step and the serving step are separate.
    #[tokio::test]
    async fn bind_does_not_accept_until_serve_is_called() {
        let server = Server::bind(&config(0)).await.unwrap();
        let addr = server.local_addr();

        let connect = TcpStream::connect(addr);
        let timed_out = tokio::time::timeout(std::time::Duration::from_millis(150), async {
            let mut stream = connect.await.unwrap();
            let mut buf = [0u8; 1];
            // The kernel completes the handshake from the backlog, so connect
            // succeeds; what must not happen is the server closing it.
            stream.read(&mut buf).await
        })
        .await;
        assert!(
            timed_out.is_err(),
            "nothing should service the connection before serve()"
        );
    }

    /// The proxy now answers rather than closing: an unusable `Host` is a 400,
    /// not a dropped connection, so the client learns why.
    #[tokio::test]
    async fn serve_answers_a_request_instead_of_closing_it() {
        let mut server = Server::bind(&config(0)).await.unwrap();
        let addr = server.local_addr();
        server.serve(proxy());

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(b"GET / HTTP/1.0\r\n\r\n").await.unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut response),
        )
        .await
        .expect("the proxy must answer promptly, not hang")
        .unwrap();

        let text = String::from_utf8_lossy(&response);
        assert!(
            text.starts_with("HTTP/1.0 400") || text.starts_with("HTTP/1.1 400"),
            "a request with no Host has no destination; got: {text:?}"
        );
        server.shutdown();
    }

    /// A connection that speaks something other than HTTP is closed, not
    /// answered, and does not take the listener down with it.
    #[tokio::test]
    async fn non_http_bytes_are_closed_and_counted() {
        let mut server = Server::bind(&config(0)).await.unwrap();
        let addr = server.local_addr();
        let proxy = proxy();
        let counters = proxy.counters();
        server.serve(Arc::clone(&proxy));

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"\x16\x03\x01\x00\xa5garbage")
            .await
            .unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut response),
        )
        .await
        .expect("garbage must be closed promptly")
        .unwrap();

        assert_eq!(
            counters.snapshot().non_http,
            1,
            "a non-HTTP connection must be counted, not silently dropped"
        );
        server.shutdown();
    }

    /// `header_timeout_ms` is the slowloris bound, and it must be real — a
    /// client that opens a connection and dribbles a header forever would
    /// otherwise hold a `max_connections` permit indefinitely.
    #[tokio::test]
    async fn a_client_that_never_finishes_its_head_is_cut_off() {
        let mut server = Server::bind(&config(0)).await.unwrap();
        let addr = server.local_addr();
        server.serve(proxy_with(Duration::from_millis(150)));

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();

        let mut response = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut response),
        )
        .await
        .expect("the header timeout must close a stalled request head")
        .unwrap();
        server.shutdown();
    }

    /// The gap p2-01 documented and could not close: with the proxy holding a
    /// permit for the life of a connection, `max_connections` finally binds.
    #[tokio::test]
    async fn the_gauge_counts_open_connections_and_keeps_the_interval_peak() {
        let mut server = Server::bind(&config(0)).await.unwrap();
        let addr = server.local_addr();
        let connections = server.connections();
        server.serve(proxy());

        let mut first = TcpStream::connect(addr).await.unwrap();
        first.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
        let mut second = TcpStream::connect(addr).await.unwrap();
        second.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
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
        let config = HttpConfig {
            max_connections: 1,
            ..config(0)
        };
        let mut server = Server::bind(&config).await.unwrap();
        assert_eq!(server.permits.available_permits(), 1);
        let addr = server.local_addr();
        server.serve(proxy());

        // Hold the only permit: connected, and deliberately sending nothing, so
        // the proxy sits waiting for a request head.
        let mut holder = TcpStream::connect(addr).await.unwrap();
        holder.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
        // Give the accept loop time to take the permit.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            server.permits.available_permits(),
            0,
            "the in-flight connection must hold the permit"
        );

        // A second connection is accepted by the kernel backlog but must not be
        // serviced while the ceiling is reached.
        let mut queued = TcpStream::connect(addr).await.unwrap();
        queued.write_all(b"GET / HTTP/1.0\r\n\r\n").await.unwrap();
        let mut buf = [0u8; 1];
        let serviced =
            tokio::time::timeout(std::time::Duration::from_millis(300), queued.read(&mut buf))
                .await;
        assert!(
            serviced.is_err(),
            "the ceiling must hold the second connection in the kernel queue"
        );

        // Release the first: the queued one is then served, so the cap
        // throttles rather than deadlocks.
        drop(holder);
        let read = tokio::time::timeout(std::time::Duration::from_secs(5), queued.read(&mut buf))
            .await
            .expect("a released permit must let the queued connection through");
        assert!(read.unwrap() > 0, "the queued connection gets a response");
        server.shutdown();
    }

    #[tokio::test]
    async fn an_invalid_listen_address_is_rejected_by_section_name() {
        let config = HttpConfig {
            listen: fah_config::HttpListenConfig {
                address: "not-an-ip".to_string(),
                port: 0,
            },
            ..HttpConfig::default()
        };
        let err = Server::bind(&config).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("[http.listen]"), "got: {err}");
    }
}

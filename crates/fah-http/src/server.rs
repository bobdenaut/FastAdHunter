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
            handle: None,
        })
    }

    /// Spawns the accept loop. Call after any privilege drop; a second call
    /// does nothing, since the listener has already been handed over.
    pub fn serve(&mut self) {
        let Some(listener) = self.listener.take() else {
            return;
        };
        let permits = Arc::clone(&self.permits);
        self.handle = Some(tokio::spawn(accept_loop(listener, permits)));
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn shutdown(&self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

/// Accepts and immediately closes (p2-01 scaffold).
///
/// The permit is acquired *before* `accept`, not after: taking the connection
/// off the queue first and then waiting would let the backlog convert into
/// in-process state, which is the bound `max_connections` exists to hold. At
/// the ceiling the loop simply stops accepting and the kernel queues, which is
/// the behaviour a client's own timeout is designed for.
async fn accept_loop(listener: TcpListener, permits: Arc<Semaphore>) {
    loop {
        let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
            // Semaphore closed — only on shutdown.
            return;
        };
        match listener.accept().await {
            Ok((stream, peer)) => {
                tokio::spawn(async move {
                    let _permit = permit;
                    handle_connection(stream, peer).await;
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

/// p2-01: close immediately. p2-02 replaces this with the proxy.
async fn handle_connection(stream: tokio::net::TcpStream, peer: SocketAddr) {
    tracing::trace!(%peer, "HTTP connection accepted and closed (scaffold)");
    drop(stream);
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt as _;
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

    #[tokio::test]
    async fn serve_accepts_then_closes_the_connection() {
        let mut server = Server::bind(&config(0)).await.unwrap();
        let addr = server.local_addr();
        server.serve();

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0u8; 1];
        let read = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf))
            .await
            .expect("the scaffold must close promptly, not hang");
        assert_eq!(
            read.unwrap(),
            0,
            "a closed connection reads EOF, not a response"
        );
        server.shutdown();
    }

    /// Verifies what is verifiable now: the pool is sized from config, and a
    /// ceiling of 1 does not stall the listener.
    ///
    /// **It does not prove the cap binds**, and cannot at this stage — the
    /// scaffold releases its permit the instant it closes, so no window exists
    /// in which a second connection could be made to wait. That test needs
    /// connections with duration and belongs to p2-02, where the proxy holds a
    /// permit for the life of a transfer. Named for what it checks rather than
    /// what the semaphore is for, so this gap stays visible.
    #[tokio::test]
    async fn max_connections_sizes_the_permit_pool_without_stalling() {
        let config = HttpConfig {
            max_connections: 1,
            ..config(0)
        };
        let mut server = Server::bind(&config).await.unwrap();
        assert_eq!(server.permits.available_permits(), 1);
        server.serve();

        // Both must complete: a ceiling of 1 throttles, it does not deadlock.
        for _ in 0..2 {
            let mut stream = TcpStream::connect(server.local_addr()).await.unwrap();
            let mut buf = [0u8; 1];
            let read =
                tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf))
                    .await
                    .expect("capped does not mean stalled");
            assert_eq!(read.unwrap(), 0);
        }
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

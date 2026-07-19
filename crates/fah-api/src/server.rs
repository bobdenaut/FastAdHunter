//! Binding and serving. HTTPS by default; `[api] tls = false` opts out to
//! plain HTTP, which SECURITY.md documents as unsafe (the API key is a bearer
//! token — one sniffed request leaks full admin control).
//!
//! The accept loop is hand-rolled rather than `axum::serve` because the
//! WebSocket route needs `serve_connection_with_upgrades`, and the TLS
//! variant has to wrap each stream in a `tokio_rustls` acceptor first.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

use crate::events::EventHub;
use crate::state::AppStateBuilder;

/// A bound, running API server.
pub struct ApiServer {
    local_addr: SocketAddr,
    tls: bool,
    events: EventHub,
    accept_loop: JoinHandle<()>,
}

impl ApiServer {
    /// Binds `[api] address:port` and starts serving. `tls_config` present
    /// means HTTPS; `None` means the documented plain-HTTP opt-out.
    pub async fn bind(
        address: &str,
        port: u16,
        tls_config: Option<Arc<rustls::ServerConfig>>,
        state: AppStateBuilder,
    ) -> io::Result<Self> {
        let addr: SocketAddr = format!("{address}:{port}").parse().map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid [api] address: {err}"),
            )
        })?;
        let listener = TcpListener::bind(addr).await?;
        // `port == 0` (tests) asks the OS for an ephemeral port — report the
        // one actually bound, mirroring `fah_dns::Server`.
        let local_addr = listener.local_addr()?;

        let events = EventHub::new();
        let router = crate::routes::router(state.build(events.clone()));
        let tls = tls_config.is_some();
        let acceptor = tls_config.map(TlsAcceptor::from);

        let accept_loop = tokio::spawn(accept(listener, router, acceptor));

        Ok(Self {
            local_addr,
            tls,
            events,
            accept_loop,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// The base URL clients should use, scheme included.
    pub fn base_url(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://{}", self.local_addr)
    }

    /// The publish side of `WS /api/v1/events`. The binary feeds completed
    /// queries in through this.
    pub fn events(&self) -> EventHub {
        self.events.clone()
    }

    pub fn shutdown(&self) {
        self.accept_loop.abort();
    }
}

async fn accept(listener: TcpListener, router: Router, acceptor: Option<TlsAcceptor>) {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(err) => {
                // A per-connection accept error (fd exhaustion, a client
                // vanishing mid-handshake) must not kill the listener.
                tracing::warn!(error = %err, "API accept failed");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
        };

        let router = router.clone();
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            if let Err(err) = serve_connection(stream, peer, router, acceptor).await {
                tracing::debug!(%peer, error = %err, "API connection ended");
            }
        });
    }
}

async fn serve_connection(
    stream: TcpStream,
    peer: SocketAddr,
    router: Router,
    acceptor: Option<TlsAcceptor>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // `into_make_service_with_connect_info` is what lets a handler ask for
    // the peer address; the router is otherwise served as a plain tower
    // service.
    let mut make_service = router.into_make_service_with_connect_info::<SocketAddr>();
    let tower_service = tower::Service::call(&mut make_service, peer).await?;
    let hyper_service = TowerToHyperService::new(tower_service);
    let builder = Builder::new(TokioExecutor::new());

    match acceptor {
        Some(acceptor) => {
            let stream = acceptor.accept(stream).await?;
            builder
                .serve_connection_with_upgrades(TokioIo::new(stream), hyper_service)
                .await
        }
        None => {
            builder
                .serve_connection_with_upgrades(TokioIo::new(stream), hyper_service)
                .await
        }
    }
}

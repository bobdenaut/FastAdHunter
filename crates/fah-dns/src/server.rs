//! Binds `[dns.listen]`'s UDP and TCP sockets and spawns their listener
//! tasks (ARCHITECTURE.md §Listeners: UDP mandatory, TCP mandatory fallback).

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use fah_common::listen::{bind_error, bind_tcp, bind_udp, listen_addr};
use fah_config::DnsListenConfig;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinHandle;

/// Named in bind failures so the operator is sent to the right setting.
const PORT_SETTING: &str = "[dns.listen] port, or FAH__DNS__LISTEN__PORT";

use crate::pipeline::Pipeline;
use crate::tcp::TcpConnectionGauge;
use crate::upstream::Forwarder;
use crate::{tcp, udp};

/// Owns the UDP + TCP listener tasks. Dropping this does not stop them (they
/// hold their own `Arc` clones of the pipeline, matching
/// `fah_rules::ListManager::spawn_scheduler`'s convention) — call
/// [`Server::shutdown`] explicitly.
pub struct Server {
    udp_addr: SocketAddr,
    tcp_addr: SocketAddr,
    /// Bound, not yet accepting — taken by [`Server::serve`].
    sockets: Option<(UdpSocket, TcpListener)>,
    tcp_permits: Arc<Semaphore>,
    tcp_gauge: Arc<TcpConnectionGauge>,
    handles: Vec<JoinHandle<()>>,
    fatal_tx: mpsc::Sender<ListenerDied>,
    fatal_rx: mpsc::Receiver<ListenerDied>,
}

impl Server {
    /// Binds both listeners **without** accepting anything yet.
    ///
    /// Binding and serving are deliberately separate: port 53 requires
    /// privilege, answering queries must not have it (ADR-0004). The caller
    /// binds, drops privileges, then calls [`Server::serve`] — so no query is
    /// ever processed by a privileged process.
    pub async fn bind(listen: &DnsListenConfig, tcp_max_connections: usize) -> io::Result<Self> {
        let addr = listen_addr(&listen.address, listen.port, "dns.listen")?;

        let udp_socket = bind_udp(addr)
            .await
            .map_err(|err| bind_error("UDP", addr, err, PORT_SETTING))?;
        let tcp_listener = bind_tcp(addr)
            .await
            .map_err(|err| bind_error("TCP", addr, err, PORT_SETTING))?;
        // `listen.port == 0` (tests only — production always pins port 53)
        // asks the OS for an ephemeral port independently per socket type, so
        // UDP and TCP can land on different numbers; record each actual bound
        // address rather than assuming they match `addr`.
        let udp_addr = udp_socket.local_addr()?;
        let tcp_addr = tcp_listener.local_addr()?;

        let (fatal_tx, fatal_rx) = mpsc::channel(1);

        Ok(Self {
            udp_addr,
            tcp_addr,
            sockets: Some((udp_socket, tcp_listener)),
            tcp_permits: Arc::new(Semaphore::new(tcp_max_connections)),
            tcp_gauge: Arc::new(TcpConnectionGauge::default()),
            handles: Vec::new(),
            fatal_tx,
            fatal_rx,
        })
    }

    /// Spawns the listener tasks. Call after any privilege drop; a second call
    /// does nothing, since the sockets have already been handed over.
    pub fn serve<F: Forwarder>(&mut self, pipeline: Arc<Pipeline<F>>) {
        let Some((udp_socket, tcp_listener)) = self.sockets.take() else {
            return;
        };
        let udp_fatal = self.fatal_tx.clone();
        let udp_pipeline = Arc::clone(&pipeline);
        self.handles.push(tokio::spawn(async move {
            let died = udp::run(udp_socket, udp_pipeline).await;
            let _ = udp_fatal.try_send(died);
        }));

        let tcp_fatal = self.fatal_tx.clone();
        let tcp_permits = Arc::clone(&self.tcp_permits);
        let tcp_gauge = Arc::clone(&self.tcp_gauge);
        self.handles.push(tokio::spawn(async move {
            let died = tcp::run(tcp_listener, pipeline, tcp_permits, tcp_gauge).await;
            let _ = tcp_fatal.try_send(died);
        }));
    }

    pub fn tcp_connections(&self) -> Arc<TcpConnectionGauge> {
        Arc::clone(&self.tcp_gauge)
    }

    pub async fn fatal(&mut self) -> ListenerDied {
        match self.fatal_rx.recv().await {
            Some(died) => died,
            None => std::future::pending().await,
        }
    }

    pub fn udp_addr(&self) -> SocketAddr {
        self.udp_addr
    }

    pub fn tcp_addr(&self) -> SocketAddr {
        self.tcp_addr
    }

    pub fn shutdown(&self) {
        for handle in &self.handles {
            handle.abort();
        }
    }
}

#[derive(Debug)]
pub struct ListenerDied {
    pub last_error: io::Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The DNS engine must keep sending operators to *its own* setting when a
    /// bind fails — the shared helper is parameterized, so this pins the
    /// argument rather than the message (which `fah-common` tests).
    #[test]
    fn a_privileged_port_failure_names_the_dns_listen_setting() {
        let addr: SocketAddr = "0.0.0.0:53".parse().unwrap();
        let text = bind_error(
            "UDP",
            addr,
            io::Error::from(io::ErrorKind::PermissionDenied),
            PORT_SETTING,
        )
        .to_string();
        assert!(text.contains("FAH__DNS__LISTEN__PORT"), "got: {text}");
        assert!(text.contains("CAP_NET_BIND_SERVICE"), "got: {text}");
    }
}

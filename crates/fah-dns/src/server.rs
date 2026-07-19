//! Binds `[dns.listen]`'s UDP and TCP sockets and spawns their listener
//! tasks (ARCHITECTURE.md §Listeners: UDP mandatory, TCP mandatory fallback).

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use fah_config::DnsListenConfig;
use tokio::net::{TcpListener, UdpSocket};
use tokio::task::JoinHandle;

use crate::pipeline::Pipeline;
use crate::upstream::Forwarder;
use crate::{tcp, udp};

/// Owns the UDP + TCP listener tasks. Dropping this does not stop them (they
/// hold their own `Arc` clones of the pipeline, matching
/// `fah_rules::ListManager::spawn_scheduler`'s convention) — call
/// [`Server::shutdown`] explicitly.
pub struct Server {
    udp_addr: SocketAddr,
    tcp_addr: SocketAddr,
    handles: Vec<JoinHandle<()>>,
}

impl Server {
    pub async fn bind<F: Forwarder>(
        listen: &DnsListenConfig,
        pipeline: Arc<Pipeline<F>>,
    ) -> io::Result<Self> {
        let addr: SocketAddr = format!("{}:{}", listen.address, listen.port)
            .parse()
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid [dns.listen] address: {err}"),
                )
            })?;

        let udp_socket = UdpSocket::bind(addr).await?;
        let tcp_listener = TcpListener::bind(addr).await?;
        // `listen.port == 0` (tests only — production always pins port 53)
        // asks the OS for an ephemeral port independently per socket type, so
        // UDP and TCP can land on different numbers; record each actual bound
        // address rather than assuming they match `addr`.
        let udp_addr = udp_socket.local_addr()?;
        let tcp_addr = tcp_listener.local_addr()?;

        let udp_handle = tokio::spawn(udp::run(udp_socket, Arc::clone(&pipeline)));
        let tcp_handle = tokio::spawn(tcp::run(tcp_listener, pipeline));

        Ok(Self {
            udp_addr,
            tcp_addr,
            handles: vec![udp_handle, tcp_handle],
        })
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

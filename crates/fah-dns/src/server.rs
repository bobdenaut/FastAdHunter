//! Binds `[dns.listen]`'s UDP and TCP sockets and spawns their listener
//! tasks (ARCHITECTURE.md §Listeners: UDP mandatory, TCP mandatory fallback).

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use fah_common::listen::{bind_error, bind_tcp, bind_udp, listen_addr};
use fah_config::DnsListenConfig;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Named in bind failures so the operator is sent to the right setting.
const PORT_SETTING: &str = "[dns.listen] port, or FAH__DNS__LISTEN__PORT";

const DOT_PORT_SETTING: &str = "[dns.listen] dot_port, or FAH__DNS__LISTEN__DOT_PORT";

use crate::dot::{self, DotTls};
use crate::pipeline::Pipeline;
use crate::upstream::Forwarder;
use crate::{tcp, udp};

struct Bound {
    udp: UdpSocket,
    tcp: TcpListener,
    dot: Option<TcpListener>,
}

pub struct Server {
    udp_addr: SocketAddr,
    tcp_addr: SocketAddr,
    dot_addr: Option<SocketAddr>,
    /// Bound, not yet accepting — taken by [`Server::serve`].
    sockets: Option<Bound>,
    handles: Vec<JoinHandle<()>>,
    fatal_tx: mpsc::Sender<ListenerDied>,
    fatal_rx: mpsc::Receiver<ListenerDied>,
}

impl Server {
    pub async fn bind(listen: &DnsListenConfig) -> io::Result<Self> {
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

        let dot_listener = match listen.dot_enabled {
            true => {
                let dot_addr = SocketAddr::new(addr.ip(), listen.dot_port);
                Some(
                    bind_tcp(dot_addr)
                        .await
                        .map_err(|err| bind_error("DoT", dot_addr, err, DOT_PORT_SETTING))?,
                )
            }
            false => None,
        };
        let dot_addr = match &dot_listener {
            Some(listener) => Some(listener.local_addr()?),
            None => None,
        };

        let (fatal_tx, fatal_rx) = mpsc::channel(1);

        Ok(Self {
            udp_addr,
            tcp_addr,
            dot_addr,
            sockets: Some(Bound {
                udp: udp_socket,
                tcp: tcp_listener,
                dot: dot_listener,
            }),
            handles: Vec::new(),
            fatal_tx,
            fatal_rx,
        })
    }

    /// Spawns the listener tasks. Call after any privilege drop; a second call
    /// does nothing, since the sockets have already been handed over.
    pub fn serve<F: Forwarder>(&mut self, pipeline: Arc<Pipeline<F>>, dot: Option<DotTls>) {
        let Some(bound) = self.sockets.take() else {
            return;
        };
        let udp_fatal = self.fatal_tx.clone();
        let udp_pipeline = Arc::clone(&pipeline);
        self.handles.push(tokio::spawn(async move {
            let died = udp::run(bound.udp, udp_pipeline).await;
            let _ = udp_fatal.try_send(died);
        }));

        let tcp_fatal = self.fatal_tx.clone();
        let tcp_pipeline = Arc::clone(&pipeline);
        self.handles.push(tokio::spawn(async move {
            let died = tcp::run(bound.tcp, tcp_pipeline).await;
            let _ = tcp_fatal.try_send(died);
        }));

        match (bound.dot, dot) {
            (Some(listener), Some(tls)) => {
                let dot_fatal = self.fatal_tx.clone();
                self.handles.push(tokio::spawn(async move {
                    let died = dot::run(listener, tls, pipeline).await;
                    let _ = dot_fatal.try_send(died);
                }));
            }
            (Some(listener), None) => {
                tracing::error!(
                    addr = ?listener.local_addr().ok(),
                    "DoT listener closed: no TLS configuration was supplied"
                );
                self.dot_addr = None;
            }
            (None, _) => {}
        }
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

    pub fn dot_addr(&self) -> Option<SocketAddr> {
        self.dot_addr
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

    #[test]
    fn a_dot_bind_failure_names_the_dot_port_setting() {
        let addr: SocketAddr = "0.0.0.0:853".parse().unwrap();
        let text = bind_error(
            "DoT",
            addr,
            io::Error::from(io::ErrorKind::PermissionDenied),
            DOT_PORT_SETTING,
        )
        .to_string();
        assert!(text.contains("binding DoT 0.0.0.0:853"), "got: {text}");
        assert!(text.contains("FAH__DNS__LISTEN__DOT_PORT"), "got: {text}");
    }
}

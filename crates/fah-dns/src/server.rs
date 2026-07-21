//! Binds `[dns.listen]`'s UDP and TCP sockets and spawns their listener
//! tasks (ARCHITECTURE.md §Listeners: UDP mandatory, TCP mandatory fallback).

use std::io;
use std::net::{IpAddr, SocketAddr};
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
    /// Bound, not yet accepting — taken by [`Server::serve`].
    sockets: Option<(UdpSocket, TcpListener)>,
    handles: Vec<JoinHandle<()>>,
}

impl Server {
    /// Binds both listeners **without** accepting anything yet.
    ///
    /// Binding and serving are deliberately separate: port 53 requires
    /// privilege, answering queries must not have it (ADR-0004). The caller
    /// binds, drops privileges, then calls [`Server::serve`] — so no query is
    /// ever processed by a privileged process.
    pub async fn bind(listen: &DnsListenConfig) -> io::Result<Self> {
        // Parsed as an IP, not via `"{addr}:{port}"` string assembly — an
        // IPv6 literal needs brackets in socket-address syntax, so the
        // round-trip through a string would reject `::` as `:::53`.
        let ip: IpAddr = listen.address.parse().map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid [dns.listen] address: {err}"),
            )
        })?;
        let addr = SocketAddr::new(ip, listen.port);

        let udp_socket = bind_udp(addr)
            .await
            .map_err(|err| bind_error("UDP", addr, err))?;
        let tcp_listener = bind_tcp(addr)
            .await
            .map_err(|err| bind_error("TCP", addr, err))?;
        // `listen.port == 0` (tests only — production always pins port 53)
        // asks the OS for an ephemeral port independently per socket type, so
        // UDP and TCP can land on different numbers; record each actual bound
        // address rather than assuming they match `addr`.
        let udp_addr = udp_socket.local_addr()?;
        let tcp_addr = tcp_listener.local_addr()?;

        Ok(Self {
            udp_addr,
            tcp_addr,
            sockets: Some((udp_socket, tcp_listener)),
            handles: Vec::new(),
        })
    }

    /// Spawns the listener tasks. Call after any privilege drop; a second call
    /// does nothing, since the sockets have already been handed over.
    pub fn serve<F: Forwarder>(&mut self, pipeline: Arc<Pipeline<F>>) {
        let Some((udp_socket, tcp_listener)) = self.sockets.take() else {
            return;
        };
        self.handles
            .push(tokio::spawn(udp::run(udp_socket, Arc::clone(&pipeline))));
        self.handles
            .push(tokio::spawn(tcp::run(tcp_listener, pipeline)));
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

/// Whether this bind address means "both stacks": `::` serves IPv4 and IPv6
/// on one dual-stack socket (CONFIGURATION.md `[dns.listen]`). Only the
/// unspecified v6 address qualifies — a concrete v6 address binds v6 alone.
fn dual_stack(addr: SocketAddr) -> bool {
    matches!(addr.ip(), IpAddr::V6(v6) if v6.is_unspecified())
}

/// tokio's `bind`, except that for `::` the socket is made dual-stack
/// *explicitly* (`IPV6_V6ONLY` off) instead of inheriting the host's
/// `net.ipv6.bindv6only` — the deployment must not change behavior with a
/// sysctl. IPv4 peers then arrive as v4-mapped addresses, which the
/// pipeline canonicalizes back to plain IPv4.
async fn bind_udp(addr: SocketAddr) -> io::Result<UdpSocket> {
    if !dual_stack(addr) {
        return UdpSocket::bind(addr).await;
    }
    let socket = socket2::Socket::new(
        socket2::Domain::IPV6,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    socket.set_only_v6(false)?;
    socket.bind(&addr.into())?;
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

async fn bind_tcp(addr: SocketAddr) -> io::Result<TcpListener> {
    if !dual_stack(addr) {
        return TcpListener::bind(addr).await;
    }
    let socket = socket2::Socket::new(
        socket2::Domain::IPV6,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    socket.set_only_v6(false)?;
    // Match what tokio's own TcpListener::bind sets: SO_REUSEADDR on Unix
    // (fast restart out of TIME_WAIT; deliberately not set on Windows, where
    // it means something less safe) and its default accept backlog.
    #[cfg(unix)]
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    TcpListener::from_std(socket.into())
}

/// Names the socket that failed and, for the two failures that actually
/// happen in the field, says which one it is.
///
/// They need opposite fixes and a bare errno does not distinguish them:
/// `EACCES` means the process may not bind a privileged port (ADR-0004 — the
/// RB5009 hits this because RouterOS honours the image's non-root user,
/// grants no `CAP_NET_BIND_SERVICE`, and unlike Docker does not lower
/// `net.ipv4.ip_unprivileged_port_start`), while `EADDRINUSE` means something
/// else in this network namespace already holds the port. Reading the first
/// as the second sends an operator hunting a conflict that does not exist.
fn bind_error(proto: &str, addr: SocketAddr, err: io::Error) -> io::Error {
    let hint = match err.kind() {
        io::ErrorKind::PermissionDenied => {
            " — a port below 1024 needs CAP_NET_BIND_SERVICE, a runtime that \
             lowers net.ipv4.ip_unprivileged_port_start, or a port above 1023 \
             ([dns.listen] port, or FAH__DNS__LISTEN__PORT)"
        }
        io::ErrorKind::AddrInUse => " — another process in this network namespace already holds it",
        _ => "",
    };
    io::Error::new(err.kind(), format!("binding {proto} {addr}: {err}{hint}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(kind: io::ErrorKind) -> String {
        let addr: SocketAddr = "0.0.0.0:53".parse().unwrap();
        bind_error("UDP", addr, io::Error::new(kind, "os says no")).to_string()
    }

    /// The distinction gate 2 of ADR-0004 turns on: a permission failure must
    /// never read as a port conflict.
    #[test]
    fn permission_denied_names_the_privilege_problem_not_a_conflict() {
        let text = message(io::ErrorKind::PermissionDenied);
        assert!(text.contains("binding UDP 0.0.0.0:53"), "got: {text}");
        assert!(text.contains("CAP_NET_BIND_SERVICE"), "got: {text}");
        assert!(!text.contains("already holds"), "got: {text}");
    }

    #[test]
    fn address_in_use_names_the_conflict_not_the_privilege() {
        let text = message(io::ErrorKind::AddrInUse);
        assert!(text.contains("already holds"), "got: {text}");
        assert!(!text.contains("CAP_NET_BIND_SERVICE"), "got: {text}");
    }

    /// An unexpected errno still gets the socket and address it failed on.
    #[test]
    fn other_errors_are_still_located() {
        let text = message(io::ErrorKind::AddrNotAvailable);
        assert!(
            text.contains("binding UDP 0.0.0.0:53: os says no"),
            "got: {text}"
        );
    }

    #[test]
    fn the_error_kind_survives_the_added_context() {
        let addr: SocketAddr = "0.0.0.0:53".parse().unwrap();
        let err = bind_error(
            "TCP",
            addr,
            io::Error::from(io::ErrorKind::PermissionDenied),
        );
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }
}

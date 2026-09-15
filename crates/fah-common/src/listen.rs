//! Dual-stack listener binding, shared by every engine that opens a socket.
//!
//! Lives at L1 because the DNS and HTTP engines are L3 siblings that may not
//! import each other (ARCHITECTURE.md §Dependency Layering), and they must not
//! disagree about what follows.
//!
//! **`IPV6_V6ONLY` is cleared explicitly.** Binding `::` serves IPv4 and IPv6
//! on one socket by our decision, never by inheriting the host's
//! `net.ipv6.bindv6only` — a deployment must not change which address families
//! it answers because of a sysctl. If one engine did this and the other did
//! not, IPv6 clients would reach one and silently not the other, which is
//! precisely the class of bug `docs/deploy-rb5009.md` §5 spends a page on.

use std::io;
use std::net::{IpAddr, SocketAddr};

use tokio::net::{TcpListener, UdpSocket};

/// Accept backlog for TCP listeners — tokio's own `TcpListener::bind` default,
/// matched so the socket2 path and the plain path behave identically.
const TCP_BACKLOG: i32 = 1024;

/// Parses a `[section]`'s `address` + `port` into a [`SocketAddr`].
///
/// The address is parsed as an IP and combined, **not** assembled as
/// `"{address}:{port}"` — an IPv6 literal needs brackets in socket-address
/// syntax, so the string round-trip would reject `::` as `:::53`.
pub fn listen_addr(address: &str, port: u16, section: &str) -> io::Result<SocketAddr> {
    let ip: IpAddr = address.parse().map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid [{section}] address: {err}"),
        )
    })?;
    Ok(SocketAddr::new(ip, port))
}

/// Whether this bind address means "both stacks": `::` serves IPv4 and IPv6 on
/// one dual-stack socket. Only the unspecified v6 address qualifies — a
/// concrete v6 address binds v6 alone.
pub fn dual_stack(addr: SocketAddr) -> bool {
    matches!(addr.ip(), IpAddr::V6(v6) if v6.is_unspecified())
}

/// tokio's `UdpSocket::bind`, except that `::` is made dual-stack explicitly.
/// IPv4 peers then arrive as v4-mapped addresses, which callers canonicalize
/// back to plain IPv4 before reporting them.
pub async fn bind_udp(addr: SocketAddr) -> io::Result<UdpSocket> {
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

/// tokio's `TcpListener::bind`, except that `::` is made dual-stack explicitly.
pub async fn bind_tcp(addr: SocketAddr) -> io::Result<TcpListener> {
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
    socket.listen(TCP_BACKLOG)?;
    socket.set_nonblocking(true)?;
    TcpListener::from_std(socket.into())
}

/// Names the socket that failed and, for the two failures that actually happen
/// in the field, says which one it is.
///
/// They need opposite fixes and a bare errno does not distinguish them:
/// `EACCES` means the process may not bind a privileged port (ADR-0004 — the
/// RB5009 hits this because RouterOS honours the image's non-root user, grants
/// no `CAP_NET_BIND_SERVICE`, and unlike Docker does not lower
/// `net.ipv4.ip_unprivileged_port_start`), while `EADDRINUSE` means something
/// else in this network namespace already holds the port. Reading the first as
/// the second sends an operator hunting a conflict that does not exist.
///
/// `port_setting` names the config key to change, e.g.
/// `"[dns.listen] port, or FAH__DNS__LISTEN__PORT"`.
pub fn bind_error(proto: &str, addr: SocketAddr, err: io::Error, port_setting: &str) -> io::Error {
    let hint = match err.kind() {
        io::ErrorKind::PermissionDenied => format!(
            " — a port below 1024 needs CAP_NET_BIND_SERVICE, a runtime that \
             lowers net.ipv4.ip_unprivileged_port_start, or a port above 1023 \
             ({port_setting})"
        ),
        io::ErrorKind::AddrInUse => format!(
            " — already in use, by another listener of this process or by another \
             process in this network namespace ({port_setting})"
        ),
        _ => String::new(),
    };
    io::Error::new(err.kind(), format!("binding {proto} {addr}: {err}{hint}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(kind: io::ErrorKind) -> String {
        let addr: SocketAddr = "0.0.0.0:53".parse().unwrap();
        bind_error(
            "UDP",
            addr,
            io::Error::new(kind, "os says no"),
            "[dns.listen] port, or FAH__DNS__LISTEN__PORT",
        )
        .to_string()
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
        assert!(text.contains("already in use"), "got: {text}");
        assert!(!text.contains("CAP_NET_BIND_SERVICE"), "got: {text}");
    }

    #[test]
    fn address_in_use_keeps_the_config_setting_and_does_not_blame_a_foreign_process() {
        let text = message(io::ErrorKind::AddrInUse);
        assert!(text.contains("FAH__DNS__LISTEN__PORT"), "got: {text}");
        assert!(
            text.contains("another listener of this process"),
            "got: {text}"
        );
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
            "[http.listen] port",
        );
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    /// The hint must name the caller's own setting — an HTTP bind failure that
    /// told the operator to change `[dns.listen]` would send them to the wrong
    /// file.
    #[test]
    fn the_hint_names_the_callers_own_setting() {
        let addr: SocketAddr = "[::]:80".parse().unwrap();
        let text = bind_error(
            "TCP",
            addr,
            io::Error::from(io::ErrorKind::PermissionDenied),
            "[http.listen] port, or FAH__HTTP__LISTEN__PORT",
        )
        .to_string();
        assert!(text.contains("FAH__HTTP__LISTEN__PORT"), "got: {text}");
        assert!(!text.contains("DNS"), "got: {text}");
    }

    /// `::` is the only address that means "both stacks"; a concrete v6
    /// address binds v6 alone and an v4 address is not dual-stack at all.
    #[test]
    fn only_the_unspecified_v6_address_is_dual_stack() {
        assert!(dual_stack("[::]:53".parse().unwrap()));
        assert!(!dual_stack("[::1]:53".parse().unwrap()));
        assert!(!dual_stack("0.0.0.0:53".parse().unwrap()));
        assert!(!dual_stack("192.168.1.1:53".parse().unwrap()));
    }

    /// The reason this is not `format!("{address}:{port}").parse()`: an IPv6
    /// literal needs brackets, so the string round-trip rejects `::`.
    #[test]
    fn a_bare_ipv6_literal_parses_without_brackets() {
        let addr = listen_addr("::", 53, "dns.listen").unwrap();
        assert_eq!(addr, "[::]:53".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn an_invalid_address_names_the_section_it_came_from() {
        let err = listen_addr("not-an-ip", 80, "http.listen").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("[http.listen]"), "got: {err}");
    }
}

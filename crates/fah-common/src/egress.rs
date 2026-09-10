//! Egress destination policy — where the proxies are allowed to connect.
//!
//! The router dst-nats port 80 into the container, and RouterOS exposes no
//! `SO_ORIGINAL_DST`-style metadata, so the only statement of where a client
//! *meant* to go is the `Host` header — which the client writes. Any LAN
//! device, or malware on one, therefore picks the proxy's destination:
//! `Host: 172.17.0.2:8443` reaches our own API, `Host: 192.168.10.1` reaches
//! the router, and cloud-metadata or link-local addresses follow the same way.
//! Without a second source of truth to cross-check the claim, the only
//! defence is an allow-policy on where we are willing to connect.
//!
//! **The policy judges a resolved [`SocketAddr`], never a hostname.** Checking
//! the name would be defeated by a DNS rebind: a public name whose A record is
//! `192.168.10.1` passes any string-level test and then connects to the router.
//! Resolve first, judge second.
//!
//! It lives at L1 and knows nothing about HTTP because Phase 3 shares it
//! verbatim — HTTPS derives its destination from SNI, an equally
//! attacker-controlled claim with the same missing `SO_ORIGINAL_DST`. The
//! protocol-local half (a missing or duplicated `Host`, an absent SNI) stays in
//! the engine that can parse those; only this address decision is common.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;

/// Why a destination was refused. Each variant's [`Refusal::reason`] is a
/// stable label — it ends up on a metric, so it must not be a formatted
/// sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Not the port this proxy intercepts.
    PortNotIntercepted,
    /// `0.0.0.0` / `::`. On Linux connecting here reaches localhost.
    Unspecified,
    Loopback,
    /// 169.254/16 and fe80::/10 — includes the cloud metadata address.
    LinkLocal,
    /// RFC 1918, plus IPv6 unique-local (fc00::/7). Covers the LAN, the
    /// router and the container subnet.
    Private,
    /// 100.64/10 — carrier NAT, and a common "looks public" hiding place.
    Cgnat,
    Multicast,
    Broadcast,
}

impl Refusal {
    /// Stable, low-cardinality label for metrics and logs.
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::PortNotIntercepted => "port_not_intercepted",
            Refusal::Unspecified => "unspecified",
            Refusal::Loopback => "loopback",
            Refusal::LinkLocal => "link_local",
            Refusal::Private => "private",
            Refusal::Cgnat => "cgnat",
            Refusal::Multicast => "multicast",
            Refusal::Broadcast => "broadcast",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason())
    }
}

/// One entry of the operator's allow-list: an address or a CIDR block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllowedNet {
    addr: IpAddr,
    prefix_len: u8,
}

impl AllowedNet {
    /// A single host.
    pub fn host(addr: IpAddr) -> Self {
        let prefix_len = if addr.is_ipv4() { 32 } else { 128 };
        Self { addr, prefix_len }
    }

    pub fn contains(&self, candidate: IpAddr) -> bool {
        match (self.addr, candidate) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => prefix_matches(
                u32::from(net).into(),
                u32::from(ip).into(),
                self.prefix_len,
                32,
            ),
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                prefix_matches(u128::from(net), u128::from(ip), self.prefix_len, 128)
            }
            // Families never match across: a v4 exception must not silently
            // authorise a v6 destination.
            _ => false,
        }
    }
}

impl FromStr for AllowedNet {
    type Err = String;

    /// `"192.168.10.50"`, `"192.168.10.0/24"`, `"fd00::/8"`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (addr_text, prefix_text) = match text.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (text, None),
        };
        let addr: IpAddr = addr_text
            .parse()
            .map_err(|_| format!("{addr_text:?} is not an IP address"))?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix_len = match prefix_text {
            None => max,
            Some(prefix) => {
                let value: u8 = prefix
                    .parse()
                    .map_err(|_| format!("{prefix:?} is not a prefix length"))?;
                if value > max {
                    return Err(format!("prefix /{value} exceeds /{max} for {addr}"));
                }
                value
            }
        };
        Ok(Self { addr, prefix_len })
    }
}

fn prefix_matches(net: u128, candidate: u128, prefix_len: u8, bits: u8) -> bool {
    if prefix_len == 0 {
        return true;
    }
    let shift = bits - prefix_len;
    // `prefix_len <= bits` is guaranteed by construction, and a full-width
    // prefix would shift by 0 — never by `bits`, which would be UB-adjacent.
    (net >> shift) == (candidate >> shift)
}

/// Decides whether the proxy may connect to a resolved destination.
///
/// Default-deny: everything private, local or otherwise non-routable is
/// refused, and the operator opts specific ranges back in for the deliberate
/// case (an internal HTTP service they want filtered).
#[derive(Debug, Clone)]
pub struct DestinationPolicy {
    /// The origin port this proxy is the intercepting party for — 80 for
    /// plain HTTP. Absolute: see [`DestinationPolicy::check`].
    origin_port: u16,
    exceptions: Vec<AllowedNet>,
}

impl DestinationPolicy {
    pub fn new(origin_port: u16, exceptions: Vec<AllowedNet>) -> Self {
        Self {
            origin_port,
            exceptions,
        }
    }

    /// Judges one resolved destination.
    ///
    /// Order matters. The port is checked first and **is not subject to the
    /// exceptions**: the router only redirects the intercepted port, so a
    /// request naming any other one was never intercepted at all — honouring it
    /// would have the proxy originate traffic no client actually directed at
    /// it. The address rules come second, and those the operator may waive.
    pub fn check(&self, destination: SocketAddr) -> Result<(), Refusal> {
        if destination.port() != self.origin_port {
            return Err(Refusal::PortNotIntercepted);
        }
        // Canonicalise before judging, or every IPv4 rule below is bypassable
        // by spelling the address as IPv6: `::ffff:192.168.10.1` is the router.
        let ip = canonicalize(destination.ip());
        if self.exceptions.iter().any(|net| net.contains(ip)) {
            return Ok(());
        }
        match ip {
            IpAddr::V4(ip) => check_v4(ip),
            IpAddr::V6(ip) => check_v6(ip),
        }
    }
}

/// Reduces the IPv6 forms that embed an IPv4 address to that address, so one
/// set of IPv4 rules governs all of them.
///
/// Four prefixes carry a v4 address: `::ffff:0:0/96` (v4-mapped, what a
/// dual-stack socket reports), `::/96` (v4-compatible, deprecated but still
/// parsed), `64:ff9b::/96` (the well-known NAT64 prefix — on a network with
/// a NAT64 gateway, `64:ff9b::10.0.0.1` reaches 10.0.0.1), and `2002::/16`
/// (6to4, where the v4 address sits in octets 2..6 rather than at the end —
/// `2002:c0a8:0a01::` is 192.168.10.1 to any host with a 6to4 route).
fn canonicalize(ip: IpAddr) -> IpAddr {
    let IpAddr::V6(v6) = ip else {
        return ip;
    };
    if let Some(v4) = v6.to_ipv4_mapped() {
        return IpAddr::V4(v4);
    }
    let octets = v6.octets();
    // 6to4 embeds its v4 address high, not low, so it must be read before the
    // low-order arms below — which would otherwise see an unrelated suffix.
    if v6.segments()[0] == 0x2002 {
        return IpAddr::V4(Ipv4Addr::new(octets[2], octets[3], octets[4], octets[5]));
    }
    let embedded = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
    // `::a.b.c.d`. The embedded address's first octet must be non-zero, or this
    // arm swallows `::` and `::1` — `::1` is all-zero above the last byte, so a
    // naive test rewrites IPv6 loopback into `0.0.0.1` and reports the wrong
    // reason for it. `0.0.0.0/8` is "this network" and was never a legitimate
    // v4-compatible embedding, so excluding it costs nothing; what remains in
    // `::/96` is handled as IPv6 below.
    if octets[..12].iter().all(|byte| *byte == 0) && octets[12] != 0 {
        return IpAddr::V4(embedded);
    }
    if octets[..4] == [0x00, 0x64, 0xff, 0x9b] && octets[4..12].iter().all(|byte| *byte == 0) {
        return IpAddr::V4(embedded);
    }
    ip
}

fn check_v4(ip: Ipv4Addr) -> Result<(), Refusal> {
    // `0.0.0.0/8` as a whole is "this network"; `0.0.0.0` in particular
    // connects to localhost on Linux, which is why it is not merely private.
    if ip.is_unspecified() || ip.octets()[0] == 0 {
        return Err(Refusal::Unspecified);
    }
    if ip.is_loopback() {
        return Err(Refusal::Loopback);
    }
    if ip.is_link_local() {
        return Err(Refusal::LinkLocal);
    }
    if ip.is_broadcast() {
        return Err(Refusal::Broadcast);
    }
    if ip.is_multicast() {
        return Err(Refusal::Multicast);
    }
    // 100.64.0.0/10. Checked before `is_private` so it reports its own reason.
    let octets = ip.octets();
    if octets[0] == 100 && (64..128).contains(&octets[1]) {
        return Err(Refusal::Cgnat);
    }
    // RFC 1918 — this is what denies the LAN, the router (192.168.10.1) and
    // the container subnet (172.17.0.0/24). Deliberately not special-cased:
    // a separate hardcoded list of "our own" addresses would be one more thing
    // to drift out of date with the deployment.
    if ip.is_private() {
        return Err(Refusal::Private);
    }
    Ok(())
}

fn check_v6(ip: Ipv6Addr) -> Result<(), Refusal> {
    if ip.is_unspecified() {
        return Err(Refusal::Unspecified);
    }
    if ip.is_loopback() {
        return Err(Refusal::Loopback);
    }
    if ip.is_multicast() {
        return Err(Refusal::Multicast);
    }
    let segments = ip.segments();
    // fe80::/10.
    if segments[0] & 0xffc0 == 0xfe80 {
        return Err(Refusal::LinkLocal);
    }
    // fc00::/7 — unique-local, IPv6's answer to RFC 1918. This is the family
    // the LAN's `fd6c:…` clients live in.
    if segments[0] & 0xfe00 == 0xfc00 {
        return Err(Refusal::Private);
    }
    // Whatever is left of `::/96` once `canonicalize` has taken the embeddings
    // it recognizes — `::0.0.0.5` and its neighbours, which the v4-compatible
    // arm deliberately skips so it cannot swallow `::1`. Nothing routes there,
    // and default-deny means refusing what we cannot justify allowing rather
    // than letting the residue of a special-cased prefix through.
    if segments[..6].iter().all(|segment| *segment == 0) {
        return Err(Refusal::Unspecified);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTTP: u16 = 80;

    fn policy() -> DestinationPolicy {
        DestinationPolicy::new(HTTP, Vec::new())
    }

    fn dest(ip: &str) -> SocketAddr {
        SocketAddr::new(ip.parse().unwrap(), HTTP)
    }

    /// The whole point of the guard: a public address on the intercepted port
    /// is the only thing that passes.
    #[test]
    fn a_public_destination_is_allowed() {
        assert_eq!(policy().check(dest("93.184.216.34")), Ok(()));
        assert_eq!(policy().check(dest("2606:2800:220:1::1")), Ok(()));
    }

    #[test]
    fn our_own_infrastructure_is_refused() {
        // The three from the task, by name: our API, the router, the container
        // subnet. All RFC 1918, none special-cased.
        for addr in ["172.17.0.2", "192.168.10.1", "172.17.0.1", "10.0.0.5"] {
            assert_eq!(
                policy().check(dest(addr)),
                Err(Refusal::Private),
                "{addr} must be refused"
            );
        }
    }

    #[test]
    fn local_and_nonroutable_ranges_are_refused() {
        for (addr, expected) in [
            ("127.0.0.1", Refusal::Loopback),
            ("0.0.0.0", Refusal::Unspecified),
            ("169.254.169.254", Refusal::LinkLocal), // cloud metadata
            ("100.64.0.1", Refusal::Cgnat),
            ("255.255.255.255", Refusal::Broadcast),
            ("224.0.0.1", Refusal::Multicast),
            ("::1", Refusal::Loopback),
            // Same address written so the v4-compatible arm would claim it —
            // it must still report loopback, not a rewritten `0.0.0.1`.
            ("::0.0.0.1", Refusal::Loopback),
            ("::", Refusal::Unspecified),
            ("fe80::1", Refusal::LinkLocal),
            ("fd6c::1", Refusal::Private),
            ("ff02::1", Refusal::Multicast),
        ] {
            assert_eq!(policy().check(dest(addr)), Err(expected), "{addr}");
        }
    }

    /// The bypass that makes every IPv4 rule above worthless if missed:
    /// spelling a private v4 address as IPv6.
    #[test]
    fn ipv6_forms_embedding_an_ipv4_address_cannot_smuggle_one_past() {
        for addr in [
            "::ffff:192.168.10.1",   // v4-mapped — what a dual-stack socket reports
            "::ffff:127.0.0.1",      // v4-mapped loopback
            "::192.168.10.1",        // v4-compatible, deprecated but still parsed
            "64:ff9b::192.168.10.1", // well-known NAT64 prefix
        ] {
            let refusal = policy().check(dest(addr));
            assert!(
                matches!(refusal, Err(Refusal::Private | Refusal::Loopback)),
                "{addr} must be judged as the IPv4 address it embeds, got {refusal:?}"
            );
        }
    }

    /// 6to4 puts the IPv4 address in octets 2..6 rather than at the end, so the
    /// low-order arms above cannot see it. On a network with a 6to4 route
    /// `2002:c0a8:0a01::` is the router, and without this it was allowed.
    #[test]
    fn a_6to4_address_is_judged_as_the_ipv4_it_carries() {
        for (addr, expected) in [
            ("2002:c0a8:0a01::", Refusal::Private), // 192.168.10.1 — the router
            ("2002:ac11:0002::", Refusal::Private), // 172.17.0.2 — our own API
            ("2002:7f00:0001::", Refusal::Loopback), // 127.0.0.1
            ("2002:a9fe:a9fe::", Refusal::LinkLocal), // 169.254.169.254
        ] {
            assert_eq!(policy().check(dest(addr)), Err(expected), "{addr}");
        }
        // A 6to4 wrapper around a public address stays allowed.
        assert_eq!(policy().check(dest("2002:5db8:d822::")), Ok(()));
    }

    /// What `canonicalize` leaves of `::/96` — it skips `::0.0.0.x` so it
    /// cannot swallow `::1`, and default-deny has to catch the remainder.
    #[test]
    fn the_residue_of_the_v4_compatible_prefix_is_refused() {
        assert_eq!(policy().check(dest("::0.0.0.5")), Err(Refusal::Unspecified));
    }

    /// A DNS rebind is exactly this: the name looked fine, the address does
    /// not. Nothing here ever sees the name, which is the design.
    #[test]
    fn only_the_resolved_address_is_judged() {
        assert_eq!(policy().check(dest("192.168.10.50")), Err(Refusal::Private));
    }

    #[test]
    fn a_port_we_do_not_intercept_is_refused_even_when_public() {
        let addr = SocketAddr::new("93.184.216.34".parse().unwrap(), 8443);
        assert_eq!(policy().check(addr), Err(Refusal::PortNotIntercepted));
    }

    #[test]
    fn exceptions_admit_a_deliberate_private_destination() {
        let policy = DestinationPolicy::new(HTTP, vec!["192.168.10.50".parse().unwrap()]);
        assert_eq!(policy.check(dest("192.168.10.50")), Ok(()));
        // Its neighbours are not admitted with it.
        assert_eq!(policy.check(dest("192.168.10.51")), Err(Refusal::Private));
    }

    #[test]
    fn a_cidr_exception_admits_the_block_and_nothing_beyond_it() {
        let policy = DestinationPolicy::new(HTTP, vec!["192.168.10.0/24".parse().unwrap()]);
        assert_eq!(policy.check(dest("192.168.10.1")), Ok(()));
        assert_eq!(policy.check(dest("192.168.10.255")), Ok(()));
        assert_eq!(policy.check(dest("192.168.11.1")), Err(Refusal::Private));
    }

    /// An exception waives the address rules, never the port rule — the router
    /// only redirects the intercepted port, so anything else was never
    /// intercepted and the proxy must not originate it.
    #[test]
    fn an_exception_does_not_waive_the_port_rule() {
        let policy = DestinationPolicy::new(HTTP, vec!["192.168.10.50".parse().unwrap()]);
        let addr = SocketAddr::new("192.168.10.50".parse().unwrap(), 8443);
        assert_eq!(policy.check(addr), Err(Refusal::PortNotIntercepted));
    }

    #[test]
    fn an_exception_does_not_cross_address_families() {
        let policy = DestinationPolicy::new(HTTP, vec!["0.0.0.0/0".parse().unwrap()]);
        // A v4 catch-all must not authorise a v6 destination.
        assert_eq!(policy.check(dest("fd6c::1")), Err(Refusal::Private));
    }

    #[test]
    fn allowed_net_parses_hosts_and_blocks_and_rejects_nonsense() {
        assert!("192.168.10.50".parse::<AllowedNet>().is_ok());
        assert!("192.168.10.0/24".parse::<AllowedNet>().is_ok());
        assert!("fd00::/8".parse::<AllowedNet>().is_ok());
        assert!("not-an-ip".parse::<AllowedNet>().is_err());
        assert!("192.168.10.0/33".parse::<AllowedNet>().is_err());
        assert!("fd00::/129".parse::<AllowedNet>().is_err());
    }

    #[test]
    fn host_helper_matches_only_itself() {
        let net = AllowedNet::host("10.0.0.1".parse().unwrap());
        assert!(net.contains("10.0.0.1".parse().unwrap()));
        assert!(!net.contains("10.0.0.2".parse().unwrap()));
    }

    /// Refusal labels reach a metric, so they must stay stable and
    /// low-cardinality.
    #[test]
    fn every_refusal_has_a_distinct_stable_label() {
        let all = [
            Refusal::PortNotIntercepted,
            Refusal::Unspecified,
            Refusal::Loopback,
            Refusal::LinkLocal,
            Refusal::Private,
            Refusal::Cgnat,
            Refusal::Multicast,
            Refusal::Broadcast,
        ];
        let mut labels: Vec<&str> = all.iter().map(|refusal| refusal.reason()).collect();
        labels.sort_unstable();
        let count = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), count, "refusal labels must be distinct");
    }
}

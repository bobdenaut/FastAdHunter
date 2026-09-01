use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

const PROBE_TARGET: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 1);

pub fn probe_local_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(PROBE_TARGET).ok()?;
    let local = socket.local_addr().ok()?.ip();
    (!local.is_loopback() && !local.is_unspecified()).then_some(local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probed_address_is_never_loopback_or_unspecified() {
        if let Some(local) = probe_local_address() {
            assert!(!local.is_loopback());
            assert!(!local.is_unspecified());
        }
    }
}

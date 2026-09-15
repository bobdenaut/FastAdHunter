use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};

fn report(case: &str, result: io::Result<usize>) {
    match result {
        Ok(sent) => println!("{case:<34} Ok({sent})"),
        Err(err) => println!(
            "{case:<34} Err errno={:?} kind={:?}",
            err.raw_os_error(),
            err.kind()
        ),
    }
}

fn v4() -> io::Result<UdpSocket> {
    UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
}

fn main() -> io::Result<()> {
    println!("target: {}", std::env::consts::OS);
    let payload = [0u8; 64];
    let oversized = vec![0u8; 70_000];

    let socket = v4()?;
    let closed = SocketAddr::from((Ipv4Addr::LOCALHOST, 1));
    report(
        "unconnected, closed port, 1st",
        socket.send_to(&payload, closed),
    );
    report(
        "unconnected, closed port, 2nd",
        socket.send_to(&payload, closed),
    );
    report(
        "unconnected, closed port, 3rd",
        socket.send_to(&payload, closed),
    );

    let connected = v4()?;
    connected.connect(closed)?;
    report("connected, closed port, 1st", connected.send(&payload));
    report("connected, closed port, 2nd", connected.send(&payload));

    report(
        "unconnected, broadcast",
        v4()?.send_to(&payload, SocketAddr::from((Ipv4Addr::BROADCAST, 53))),
    );

    report(
        "unconnected, 70 000 bytes",
        v4()?.send_to(&oversized, SocketAddr::from((Ipv4Addr::LOCALHOST, 53))),
    );

    report(
        "unconnected, 0.0.0.0:53",
        v4()?.send_to(&payload, SocketAddr::from((Ipv4Addr::UNSPECIFIED, 53))),
    );

    report(
        "unconnected, 240.0.0.1 reserved",
        v4()?.send_to(
            &payload,
            SocketAddr::from((Ipv4Addr::new(240, 0, 0, 1), 53)),
        ),
    );

    match UdpSocket::bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))) {
        Ok(v6) => {
            report(
                "unconnected, 2001:db8::1 v6",
                v6.send_to(
                    &payload,
                    SocketAddr::from((Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1), 53)),
                ),
            );
            let v6_connected = UdpSocket::bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)))?;
            let v6_closed = SocketAddr::from((Ipv6Addr::LOCALHOST, 1));
            if v6_connected.connect(v6_closed).is_ok() {
                report("connected v6, closed port, 2nd", {
                    let _ = v6_connected.send(&payload);
                    v6_connected.send(&payload)
                });
            }
        }
        Err(err) => println!("v6 bind unavailable: {err}"),
    }

    Ok(())
}

//! Plain-DNS transport: one fresh UDP socket per query — with the message ID
//! randomized — and the RFC 1035 §4.2.2 TCP retry against the same server
//! when the answer comes back truncated.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use hickory_proto::op::{Message, MessageType};
use rand::Rng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::timeout;

use crate::response::max_udp_payload;

/// One full plain-DNS attempt against `upstream`: UDP first, then a TCP
/// retry to the same server if the reply arrives truncated. Each leg is
/// bounded by `attempt_timeout` on its own — a truncated-but-alive upstream
/// is not the down-primary case the fallback window budgets for.
pub(super) async fn query(
    upstream: SocketAddr,
    request: &Message,
    attempt_timeout: Duration,
) -> io::Result<Message> {
    let mut request_bytes = request
        .to_vec()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
    // Fresh random ID per upstream query — the client's own ID is attacker-
    // observable on the LAN side. Combined with the ephemeral source port
    // this is the full 32 bits an off-path spoofer must guess to poison the
    // cache p1-05 added (the header ID is the wire format's first 2 bytes).
    let id: u16 = rand::rng().random();
    request_bytes[..2].copy_from_slice(&id.to_be_bytes());

    // The reply can't legitimately exceed what the forwarded EDNS advertises
    // (512 without EDNS) — a right-sized recv buffer instead of a blanket
    // 64KiB allocation per query. A compliant upstream truncates past this;
    // a non-compliant one's oversized datagram loses its tail in the kernel,
    // fails to parse, and runs into the timeout — same as any garbage reply.
    let reply_budget = usize::from(max_udp_payload(request));

    let response =
        udp_round_trip(upstream, &request_bytes, id, reply_budget, attempt_timeout).await?;
    if !response.metadata.truncation {
        return Ok(response);
    }
    tcp_round_trip(upstream, &request_bytes, id, attempt_timeout).await
}

async fn udp_round_trip(
    upstream: SocketAddr,
    request_bytes: &[u8],
    request_id: u16,
    reply_budget: usize,
    attempt_timeout: Duration,
) -> io::Result<Message> {
    let bind_addr = if upstream.is_ipv4() {
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
    } else {
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))
    };
    let socket = UdpSocket::bind(bind_addr).await?;
    socket.connect(upstream).await?;
    socket.send(request_bytes).await?;

    // `connect` makes the kernel drop datagrams from any other source
    // address, but an off-path spoofer who guesses the ephemeral port can
    // still land one here — accepting only a well-formed *response* whose
    // ID matches ours puts the ID's 16 bits back into the attacker's
    // guess (standard resolver behavior; anything else is discarded and
    // the wait continues until the timeout).
    let receive_matching = async {
        let mut buf = vec![0u8; reply_budget];
        loop {
            let len = socket.recv(&mut buf).await?;
            if let Ok(response) = Message::from_vec(&buf[..len]) {
                if response.metadata.id == request_id
                    && response.metadata.message_type == MessageType::Response
                {
                    return Ok(response);
                }
            }
        }
    };
    timeout(attempt_timeout, receive_matching)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "upstream timed out"))?
}

/// One-shot length-prefixed exchange (RFC 1035 §4.2.2 framing, the same the
/// listener side speaks in `crate::tcp`). No connection reuse: truncation is
/// rare enough on 1232-byte-EDNS paths that holding idle TCP connections to
/// every UDP upstream would cost more than the occasional handshake.
async fn tcp_round_trip(
    upstream: SocketAddr,
    request_bytes: &[u8],
    request_id: u16,
    attempt_timeout: Duration,
) -> io::Result<Message> {
    let exchange = async {
        let mut stream = TcpStream::connect(upstream).await?;
        let request_len = u16::try_from(request_bytes.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "query exceeds 64KiB"))?;
        stream.write_all(&request_len.to_be_bytes()).await?;
        stream.write_all(request_bytes).await?;

        let mut len_buf = [0u8; 2];
        stream.read_exact(&mut len_buf).await?;
        let mut reply = vec![0u8; u16::from_be_bytes(len_buf) as usize];
        stream.read_exact(&mut reply).await?;

        let response = Message::from_vec(&reply)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        // TCP can't be off-path-spoofed like UDP, but a mismatched ID still
        // means a confused or broken upstream — not an answer to relay.
        if response.metadata.id != request_id
            || response.metadata.message_type != MessageType::Response
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "TCP upstream answered with a mismatched message",
            ));
        }
        Ok(response)
    };
    timeout(attempt_timeout, exchange)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "TCP upstream timed out"))?
}

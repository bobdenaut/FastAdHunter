//! UDP/53 listener: EDNS(0)-aware (payload size honored, truncation applied
//! when a reply exceeds it — ARCHITECTURE.md §Listeners).

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;
use tracing::warn;

use crate::pipeline::{Pipeline, Transport};
use crate::upstream::Forwarder;

/// Runs the UDP listener loop until the socket errors unrecoverably. Every
/// worker on the Tokio runtime can be running one of these — no central
/// dispatcher (ARCHITECTURE.md §Runtime Model) — but Phase 1 binds exactly
/// one socket and spawns one receive loop; each datagram's handling still
/// runs as its own task so one slow/blocked forward never delays the next
/// datagram's receipt.
pub async fn run<F: Forwarder>(socket: UdpSocket, pipeline: Arc<Pipeline<F>>) {
    let socket = Arc::new(socket);
    // Max DNS-over-UDP message size (RFC 6891 practical ceiling); anything
    // larger is not a DNS packet.
    let mut buf = [0u8; 65535];
    loop {
        let (len, client) = match socket.recv_from(&mut buf).await {
            Ok(pair) => pair,
            Err(err) => {
                warn!(error = %err, "UDP socket recv failed; stopping listener");
                return;
            }
        };
        let datagram = buf[..len].to_vec();
        let socket = Arc::clone(&socket);
        let pipeline = Arc::clone(&pipeline);
        tokio::spawn(async move {
            handle_datagram(&socket, &pipeline, &datagram, client).await;
        });
    }
}

async fn handle_datagram<F: Forwarder>(
    socket: &UdpSocket,
    pipeline: &Pipeline<F>,
    datagram: &[u8],
    client: SocketAddr,
) {
    let Some(reply) = pipeline.handle(datagram, client.ip(), Transport::Udp).await else {
        return;
    };
    if let Err(err) = socket.send_to(&reply, client).await {
        warn!(error = %err, client = %client, "failed to send UDP DNS reply");
    }
}

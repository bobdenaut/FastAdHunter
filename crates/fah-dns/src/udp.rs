//! UDP/53 listener: EDNS(0)-aware (payload size honored, truncation applied
//! when a reply exceeds it — ARCHITECTURE.md §Listeners).

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;
use tracing::warn;

use crate::backoff::{RetryDecision, RetryPolicy};
use crate::pipeline::{Pipeline, Transport};
use crate::server::ListenerDied;
use crate::upstream::Forwarder;

pub trait Datagrams: Send + Sync + 'static {
    fn recv_from(
        &self,
        buf: &mut [u8],
    ) -> impl Future<Output = io::Result<(usize, SocketAddr)>> + Send;

    fn send_to(
        &self,
        reply: &[u8],
        client: SocketAddr,
    ) -> impl Future<Output = io::Result<usize>> + Send;
}

impl Datagrams for UdpSocket {
    fn recv_from(
        &self,
        buf: &mut [u8],
    ) -> impl Future<Output = io::Result<(usize, SocketAddr)>> + Send {
        UdpSocket::recv_from(self, buf)
    }

    fn send_to(
        &self,
        reply: &[u8],
        client: SocketAddr,
    ) -> impl Future<Output = io::Result<usize>> + Send {
        UdpSocket::send_to(self, reply, client)
    }
}

pub async fn run<S: Datagrams, F: Forwarder>(
    socket: S,
    pipeline: Arc<Pipeline<F>>,
) -> ListenerDied {
    let socket = Arc::new(socket);
    let mut policy = RetryPolicy::new();
    // Max DNS-over-UDP message size (RFC 6891 practical ceiling); anything
    // larger is not a DNS packet.
    let mut buf = [0u8; 65535];
    loop {
        let (len, client) = match socket.recv_from(&mut buf).await {
            Ok(pair) => {
                policy.on_success();
                pair
            }
            Err(err) => match policy.on_error() {
                RetryDecision::Sleep(delay) => {
                    warn!(
                        error = %err,
                        retry_in_ms = delay.as_millis(),
                        "UDP socket recv failed; retrying"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                RetryDecision::Fatal => return ListenerDied { last_error: err },
            },
        };
        let datagram = buf[..len].to_vec();
        let socket = Arc::clone(&socket);
        let pipeline = Arc::clone(&pipeline);
        tokio::spawn(async move {
            handle_datagram(socket.as_ref(), &pipeline, &datagram, client).await;
        });
    }
}

async fn handle_datagram<S: Datagrams, F: Forwarder>(
    socket: &S,
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

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::backoff::FATAL_CONSECUTIVE_ERRORS;
    use crate::testkit;

    struct FlakySocket {
        errors_before_first_success: u32,
        errors: Arc<AtomicU32>,
        delivered: Arc<AtomicU32>,
    }

    impl Datagrams for FlakySocket {
        async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
            if self.errors.load(Ordering::Relaxed) == self.errors_before_first_success
                && self.delivered.load(Ordering::Relaxed) == 0
            {
                self.delivered.fetch_add(1, Ordering::Relaxed);
                buf[..2].copy_from_slice(&[0x2a, 0x2a]);
                return Ok((2, SocketAddr::from((Ipv4Addr::LOCALHOST, 5353))));
            }
            self.errors.fetch_add(1, Ordering::Relaxed);
            Err(io::Error::other("induced recv failure"))
        }

        async fn send_to(&self, reply: &[u8], _client: SocketAddr) -> io::Result<usize> {
            Ok(reply.len())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_transient_recv_error_retries_and_a_receive_resets_the_escalation() {
        let (pipeline, _data_dir) = testkit::pipeline();
        let errors = Arc::new(AtomicU32::new(0));
        let delivered = Arc::new(AtomicU32::new(0));
        let socket = FlakySocket {
            errors_before_first_success: FATAL_CONSECUTIVE_ERRORS - 1,
            errors: Arc::clone(&errors),
            delivered: Arc::clone(&delivered),
        };

        let died = run(socket, pipeline).await;

        assert!(
            died.last_error.to_string().contains("induced recv failure"),
            "got: {}",
            died.last_error
        );
        assert_eq!(delivered.load(Ordering::Relaxed), 1);
        assert_eq!(
            errors.load(Ordering::Relaxed),
            2 * FATAL_CONSECUTIVE_ERRORS - 1,
            "the successful receive must reset the consecutive-error count"
        );
    }
}

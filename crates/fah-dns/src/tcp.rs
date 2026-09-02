//! TCP/53 listener: mandatory truncation fallback (ARCHITECTURE.md
//! §Listeners) plus any request a client sends over TCP directly. RFC 1035
//! §4.2.2 length-prefixed framing: a 2-byte big-endian length, then that many
//! message bytes. Connections are reused for multiple queries (RFC 7766 §6.2.1
//! — stub resolvers pipeline over one connection) and closed after
//! [`TCP_IDLE_TIMEOUT`] without a complete request, so an idle or stalled
//! client can't hold a task and file descriptor forever (CLAUDE.md: bounded
//! everything).

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::backoff::{RetryDecision, RetryPolicy};
use crate::pipeline::{Pipeline, Transport};
use crate::server::ListenerDied;
use crate::upstream::Forwarder;

/// How long a connection may sit without delivering a complete request
/// before we close it. RFC 7766 §6.2.3 leaves the value to the server;
/// long enough for a stub resolver's think time, short enough that idle
/// connections can't accumulate on the RB5009.
pub(crate) const TCP_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

pub trait Accept: Send + Sync + 'static {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;

    fn accept(&self) -> impl Future<Output = io::Result<(Self::Stream, SocketAddr)>> + Send;
}

impl Accept for TcpListener {
    type Stream = TcpStream;

    fn accept(&self) -> impl Future<Output = io::Result<(TcpStream, SocketAddr)>> + Send {
        TcpListener::accept(self)
    }
}

pub async fn run<L: Accept, F: Forwarder>(listener: L, pipeline: Arc<Pipeline<F>>) -> ListenerDied {
    let mut policy = RetryPolicy::new();
    loop {
        let (stream, client) = match listener.accept().await {
            Ok(pair) => {
                policy.on_success();
                pair
            }
            Err(err) => match policy.on_error() {
                RetryDecision::Sleep(delay) => {
                    warn!(
                        error = %err,
                        retry_in_ms = delay.as_millis(),
                        "TCP listener accept failed; retrying"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                RetryDecision::Fatal => return ListenerDied { last_error: err },
            },
        };
        let pipeline = Arc::clone(&pipeline);
        tokio::spawn(async move {
            let served = handle_connection(stream, &pipeline, client.ip(), Transport::Tcp).await;
            report_connection_end(served, client, "TCP DNS");
        });
    }
}

pub(crate) fn report_connection_end(result: io::Result<()>, client: SocketAddr, what: &str) {
    let Err(err) = result else {
        return;
    };
    if is_client_disconnect(&err) {
        debug!(error = %err, client = %client, "{what} client disconnected");
    } else {
        warn!(error = %err, client = %client, "{what} connection ended with an error");
    }
}

/// Serves queries off one connection until the client closes it, the idle
/// timeout fires, or it sends something malformed (RFC 7766 §6.2.4 permits
/// closing on protocol errors; a client that framed garbage can't be trusted
/// to frame the next message either).
pub(crate) async fn handle_connection<S: AsyncRead + AsyncWrite + Unpin, F: Forwarder>(
    mut stream: S,
    pipeline: &Pipeline<F>,
    client_ip: std::net::IpAddr,
    transport: Transport,
) -> std::io::Result<()> {
    loop {
        let mut len_buf = [0u8; 2];
        match timeout(TCP_IDLE_TIMEOUT, stream.read_exact(&mut len_buf)).await {
            Err(_elapsed) => return Ok(()), // idle: close quietly
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Ok(Err(err)) => return Err(err),
            Ok(Ok(_)) => {}
        }
        let len = u16::from_be_bytes(len_buf) as usize;

        let mut message_buf = vec![0u8; len];
        // A stalled body after a complete length prefix is the same stalled
        // client — same clock.
        match timeout(TCP_IDLE_TIMEOUT, stream.read_exact(&mut message_buf)).await {
            Err(_elapsed) => return Ok(()),
            Ok(result) => result?,
        };

        let Some(reply) = pipeline.handle(&message_buf, client_ip, transport).await else {
            return Ok(());
        };

        let reply_len = u16::try_from(reply.len()).unwrap_or(u16::MAX).to_be_bytes();
        stream.write_all(&reply_len).await?;
        stream.write_all(&reply).await?;
    }
}

/// Whether an I/O error is just the client hanging up — the connection kinds a
/// DNS-over-TCP server sees constantly and can do nothing about, versus a real
/// fault worth a warning. `UnexpectedEof` here is a client that closed
/// mid-message (the between-message clean close is already handled as `Ok`).
fn is_client_disconnect(err: &std::io::Error) -> bool {
    use std::io::ErrorKind::{BrokenPipe, ConnectionAborted, ConnectionReset, UnexpectedEof};
    matches!(
        err.kind(),
        BrokenPipe | ConnectionReset | ConnectionAborted | UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::sync::atomic::{AtomicU32, Ordering};

    use tokio::io::DuplexStream;

    use super::*;
    use crate::backoff::FATAL_CONSECUTIVE_ERRORS;
    use crate::testkit;

    struct FlakyListener {
        errors_before_first_success: u32,
        errors: Arc<AtomicU32>,
        accepted: Arc<AtomicU32>,
    }

    impl Accept for FlakyListener {
        type Stream = DuplexStream;

        async fn accept(&self) -> io::Result<(DuplexStream, SocketAddr)> {
            if self.errors.load(Ordering::Relaxed) == self.errors_before_first_success
                && self.accepted.load(Ordering::Relaxed) == 0
            {
                self.accepted.fetch_add(1, Ordering::Relaxed);
                let (server, client) = tokio::io::duplex(64);
                drop(client);
                return Ok((server, SocketAddr::from((Ipv4Addr::LOCALHOST, 5353))));
            }
            self.errors.fetch_add(1, Ordering::Relaxed);
            Err(io::Error::other("induced accept failure"))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_transient_accept_error_retries_and_an_accept_resets_the_escalation() {
        let (pipeline, _data_dir) = testkit::pipeline();
        let errors = Arc::new(AtomicU32::new(0));
        let accepted = Arc::new(AtomicU32::new(0));
        let listener = FlakyListener {
            errors_before_first_success: FATAL_CONSECUTIVE_ERRORS - 1,
            errors: Arc::clone(&errors),
            accepted: Arc::clone(&accepted),
        };

        let died = run(listener, pipeline).await;

        assert!(
            died.last_error
                .to_string()
                .contains("induced accept failure"),
            "got: {}",
            died.last_error
        );
        assert_eq!(accepted.load(Ordering::Relaxed), 1);
        assert_eq!(
            errors.load(Ordering::Relaxed),
            2 * FATAL_CONSECUTIVE_ERRORS - 1,
            "the successful accept must reset the consecutive-error count"
        );
    }
}

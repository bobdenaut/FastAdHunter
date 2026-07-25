//! TCP/53 listener: mandatory truncation fallback (ARCHITECTURE.md
//! §Listeners) plus any request a client sends over TCP directly. RFC 1035
//! §4.2.2 length-prefixed framing: a 2-byte big-endian length, then that many
//! message bytes. Connections are reused for multiple queries (RFC 7766 §6.2.1
//! — stub resolvers pipeline over one connection) and closed after
//! [`TCP_IDLE_TIMEOUT`] without a complete request, so an idle or stalled
//! client can't hold a task and file descriptor forever (CLAUDE.md: bounded
//! everything).

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::pipeline::{Pipeline, Transport};
use crate::upstream::Forwarder;

/// How long a connection may sit without delivering a complete request
/// before we close it. RFC 7766 §6.2.3 leaves the value to the server;
/// long enough for a stub resolver's think time, short enough that idle
/// connections can't accumulate on the RB5009.
const TCP_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

/// Runs the TCP accept loop until the listener errors unrecoverably. Each
/// connection is its own task — one slow/malicious client can't stall
/// another's query (ARCHITECTURE.md §Runtime Model: no central dispatcher).
pub async fn run<F: Forwarder>(listener: TcpListener, pipeline: Arc<Pipeline<F>>) {
    loop {
        let (stream, client) = match listener.accept().await {
            Ok(pair) => pair,
            Err(err) => {
                warn!(error = %err, "TCP listener accept failed; stopping listener");
                return;
            }
        };
        let pipeline = Arc::clone(&pipeline);
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, &pipeline, client.ip()).await {
                if is_client_disconnect(&err) {
                    // A client that closes mid-query/response — broken pipe,
                    // connection reset, an early EOF — is routine for
                    // DNS-over-TCP (happy-eyeballs dropping the loser, a UDP
                    // answer that arrived first, a client timeout). Not
                    // operator-actionable, so it must not warn on the router log
                    // like a real fault. (A clean close *between* messages is
                    // already returned as `Ok` in `handle_connection`.)
                    debug!(error = %err, client = %client, "TCP DNS client disconnected");
                } else {
                    warn!(error = %err, client = %client, "TCP DNS connection ended with an error");
                }
            }
        });
    }
}

/// Serves queries off one connection until the client closes it, the idle
/// timeout fires, or it sends something malformed (RFC 7766 §6.2.4 permits
/// closing on protocol errors; a client that framed garbage can't be trusted
/// to frame the next message either).
async fn handle_connection<F: Forwarder>(
    mut stream: TcpStream,
    pipeline: &Pipeline<F>,
    client_ip: std::net::IpAddr,
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

        let Some(reply) = pipeline
            .handle(&message_buf, client_ip, Transport::Tcp)
            .await
        else {
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

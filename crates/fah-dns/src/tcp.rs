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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fah_common::connections::{ConnectionGauge, OpenConnection};
use fah_common::retry::{RetryDecision, RetryPolicy};
use fah_common::throttle::LogThrottle;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tracing::{debug, warn};

pub const MAX_MESSAGE_LEN: usize = 16 * 1024;

pub const CONNECTION_WARN_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct TcpConnectionGauge {
    connections: ConnectionGauge,
    closed_oversize: AtomicU64,
    connection_errors: LogThrottle,
    prewarm_failures: LogThrottle,
}

impl Default for TcpConnectionGauge {
    fn default() -> Self {
        Self {
            connections: ConnectionGauge::default(),
            closed_oversize: AtomicU64::new(0),
            connection_errors: LogThrottle::new(CONNECTION_WARN_INTERVAL),
            prewarm_failures: LogThrottle::new(CONNECTION_WARN_INTERVAL),
        }
    }
}

impl TcpConnectionGauge {
    pub fn snapshot(&self) -> fah_model::DnsTcpConnections {
        let active = self.connections.open();
        let peak = self.connections.peak().max(active);
        fah_model::DnsTcpConnections {
            active: u64::from(active),
            peak: u64::from(peak),
            closed_oversize: self.closed_oversize.load(Ordering::Relaxed),
        }
    }
}

impl AsRef<ConnectionGauge> for TcpConnectionGauge {
    fn as_ref(&self) -> &ConnectionGauge {
        &self.connections
    }
}

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

    async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        let (stream, client) = TcpListener::accept(self).await?;
        if let Err(err) = stream.set_nodelay(true) {
            debug!(error = %err, client = %client, "TCP_NODELAY not set on a TCP DNS connection");
        }
        Ok((stream, client))
    }
}

pub async fn run<L: Accept, F: Forwarder>(
    listener: L,
    pipeline: Arc<Pipeline<F>>,
    permits: Arc<Semaphore>,
    gauge: Arc<TcpConnectionGauge>,
) -> ListenerDied {
    let mut policy = RetryPolicy::new();
    loop {
        let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
            return ListenerDied {
                last_error: io::Error::other("TCP connection semaphore closed"),
            };
        };
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
        let open = OpenConnection::enter(&gauge);
        tokio::spawn(async move {
            let served =
                handle_connection(stream, &pipeline, client.ip(), Transport::Tcp, &open).await;
            report_connection_end(served, client, "TCP DNS", &open);
            drop(open);
            drop(permit);
        });
    }
}

pub(crate) fn report_connection_end(
    result: io::Result<()>,
    client: SocketAddr,
    what: &str,
    gauge: &TcpConnectionGauge,
) {
    let Err(err) = result else {
        return;
    };
    if is_client_disconnect(&err) {
        debug!(error = %err, client = %client, "{what} client disconnected");
        return;
    }
    match gauge.connection_errors.note(std::time::Instant::now()) {
        Some(failures) => {
            warn!(error = %err, client = %client, failures, "{what} connection ended with an error")
        }
        None => debug!(error = %err, client = %client, "{what} connection ended with an error"),
    }
}

pub(crate) fn report_prewarm_failure(
    err: &tokio::task::JoinError,
    host: &str,
    what: &str,
    gauge: &TcpConnectionGauge,
) {
    match gauge.prewarm_failures.note(std::time::Instant::now()) {
        Some(failures) => {
            warn!(host = %host, error = %err, failures, "{what} leaf pre-warm task failed")
        }
        None => debug!(host = %host, error = %err, "{what} leaf pre-warm task failed"),
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
    gauge: &TcpConnectionGauge,
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
        if len > MAX_MESSAGE_LEN {
            gauge.closed_oversize.fetch_add(1, Ordering::Relaxed);
            debug!(
                client = %client_ip,
                len,
                max = MAX_MESSAGE_LEN,
                "TCP DNS message length exceeds the bound; closing"
            );
            return Ok(());
        }

        let mut message_buf = vec![0u8; len];
        // A stalled body after a complete length prefix is the same stalled
        // client — same clock.
        match timeout(TCP_IDLE_TIMEOUT, stream.read_exact(&mut message_buf)).await {
            Err(_elapsed) => return Ok(()),
            Ok(result) => result?,
        };

        let Some(mut reply) = pipeline.handle(&message_buf, client_ip, transport).await else {
            return Ok(());
        };

        frame_reply(&mut reply);
        stream.write_all(&reply).await?;
    }
}

pub(crate) fn frame_reply(reply: &mut Vec<u8>) {
    let len = u16::try_from(reply.len()).unwrap_or(u16::MAX).to_be_bytes();
    reply.splice(0..0, len);
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
    use std::collections::VecDeque;
    use std::net::Ipv4Addr;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    use tokio::io::{duplex, DuplexStream};

    use super::*;
    use crate::testkit;
    use fah_common::retry::FATAL_CONSECUTIVE_ERRORS;

    #[test]
    fn a_reply_is_framed_as_one_buffer_with_its_big_endian_length_in_front() {
        let mut reply = vec![0xAB; 300];
        frame_reply(&mut reply);
        assert_eq!(reply.len(), 302);
        assert_eq!(&reply[..2], &300u16.to_be_bytes());
        assert!(reply[2..].iter().all(|byte| *byte == 0xAB));

        let mut empty = Vec::new();
        frame_reply(&mut empty);
        assert_eq!(empty, vec![0, 0]);

        let mut oversized = vec![0u8; usize::from(u16::MAX) + 1];
        frame_reply(&mut oversized);
        assert_eq!(&oversized[..2], &u16::MAX.to_be_bytes());
        assert_eq!(oversized.len(), usize::from(u16::MAX) + 3);
    }

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

        let died = run(
            listener,
            pipeline,
            Arc::new(Semaphore::new(1024)),
            Arc::new(TcpConnectionGauge::default()),
        )
        .await;

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

    struct QueuedListener {
        streams: Mutex<VecDeque<DuplexStream>>,
        accepted: Arc<AtomicU32>,
    }

    impl QueuedListener {
        fn new(count: usize) -> (Self, Vec<DuplexStream>, Arc<AtomicU32>) {
            let mut servers = VecDeque::new();
            let mut clients = Vec::new();
            for _ in 0..count {
                let (server, client) = duplex(64);
                servers.push_back(server);
                clients.push(client);
            }
            let accepted = Arc::new(AtomicU32::new(0));
            let listener = Self {
                streams: Mutex::new(servers),
                accepted: Arc::clone(&accepted),
            };
            (listener, clients, accepted)
        }
    }

    impl Accept for QueuedListener {
        type Stream = DuplexStream;

        async fn accept(&self) -> io::Result<(DuplexStream, SocketAddr)> {
            let next = self.streams.lock().unwrap().pop_front();
            match next {
                Some(stream) => {
                    self.accepted.fetch_add(1, Ordering::Relaxed);
                    Ok((stream, SocketAddr::from((Ipv4Addr::LOCALHOST, 5353))))
                }
                None => std::future::pending().await,
            }
        }
    }

    async fn settle() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn a_length_prefix_over_the_bound_closes_the_connection_and_counts() {
        let (pipeline, _data_dir) = testkit::pipeline();
        let (listener, mut clients, _accepted) = QueuedListener::new(1);
        let gauge = Arc::new(TcpConnectionGauge::default());
        let server = tokio::spawn(run(
            listener,
            pipeline,
            Arc::new(Semaphore::new(1024)),
            Arc::clone(&gauge),
        ));

        let mut client = clients.pop().unwrap();
        let oversize = ((MAX_MESSAGE_LEN + 1) as u16).to_be_bytes();
        client.write_all(&oversize).await.unwrap();
        let mut sink = [0u8; 1];
        let read = client.read(&mut sink).await.unwrap();

        assert_eq!(read, 0, "the server must close without answering");
        let snapshot = gauge.snapshot();
        assert_eq!(snapshot.closed_oversize, 1);
        assert_eq!(snapshot.active, 0);
        assert_eq!(snapshot.peak, 1);
        server.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_length_prefix_at_the_bound_is_read_not_rejected() {
        let (pipeline, _data_dir) = testkit::pipeline();
        let (listener, mut clients, _accepted) = QueuedListener::new(1);
        let gauge = Arc::new(TcpConnectionGauge::default());
        let server = tokio::spawn(run(
            listener,
            pipeline,
            Arc::new(Semaphore::new(1024)),
            Arc::clone(&gauge),
        ));

        let mut client = clients.pop().unwrap();
        let at_bound = (MAX_MESSAGE_LEN as u16).to_be_bytes();
        client.write_all(&at_bound).await.unwrap();
        settle().await;

        assert_eq!(gauge.snapshot().closed_oversize, 0);
        assert_eq!(gauge.snapshot().active, 1, "still waiting for the body");
        server.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn the_connection_ceiling_holds_the_next_accept_until_one_closes() {
        let (pipeline, _data_dir) = testkit::pipeline();
        let (listener, mut clients, accepted) = QueuedListener::new(2);
        let gauge = Arc::new(TcpConnectionGauge::default());
        let server = tokio::spawn(run(
            listener,
            pipeline,
            Arc::new(Semaphore::new(1)),
            Arc::clone(&gauge),
        ));

        let second = clients.pop().unwrap();
        let first = clients.pop().unwrap();
        settle().await;
        assert_eq!(accepted.load(Ordering::Relaxed), 1);
        assert_eq!(gauge.snapshot().active, 1);

        drop(first);
        settle().await;
        assert_eq!(
            accepted.load(Ordering::Relaxed),
            2,
            "closing the first connection frees the permit"
        );
        let snapshot = gauge.snapshot();
        assert_eq!(snapshot.active, 1);
        assert_eq!(snapshot.peak, 1, "never two at once under a ceiling of one");

        drop(second);
        settle().await;
        assert_eq!(gauge.snapshot().active, 0);
        server.abort();
    }
}

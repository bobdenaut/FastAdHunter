//! UDP/53 listener: EDNS(0)-aware (payload size honored, truncation applied
//! when a reply exceeds it — ARCHITECTURE.md §Listeners).

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fah_common::retry::{RetryDecision, RetryPolicy};
use fah_common::throttle::LogThrottle;
use tokio::net::UdpSocket;
use tracing::{debug, warn};

use crate::pipeline::{Pipeline, Transport};
use crate::server::ListenerDied;
use crate::upstream::Forwarder;

pub const SEND_FAILURE_WARN_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct UdpInflightGauge {
    limit: Option<NonZeroUsize>,
    active: AtomicUsize,
    peak: AtomicUsize,
    shed: AtomicU64,
    send_failures: LogThrottle,
}

impl UdpInflightGauge {
    pub fn new(max_inflight: usize) -> Self {
        Self {
            limit: NonZeroUsize::new(max_inflight),
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            shed: AtomicU64::new(0),
            send_failures: LogThrottle::new(SEND_FAILURE_WARN_INTERVAL),
        }
    }

    fn admit(&self) -> bool {
        let Some(limit) = self.limit else {
            return true;
        };
        let mut active = self.active.load(Ordering::Relaxed);
        loop {
            if active >= limit.get() {
                self.shed.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            match self.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.peak.fetch_max(active + 1, Ordering::Relaxed);
                    return true;
                }
                Err(current) => active = current,
            }
        }
    }

    fn release(&self) {
        if self.limit.is_some() {
            self.active.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> fah_model::DnsUdpInflight {
        let active = self.active.load(Ordering::Relaxed);
        fah_model::DnsUdpInflight {
            active: as_u64(active),
            peak: as_u64(self.peak.load(Ordering::Relaxed).max(active)),
            shed: self.shed.load(Ordering::Relaxed),
        }
    }
}

fn as_u64(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

struct Listener<S> {
    socket: S,
    gauge: Arc<UdpInflightGauge>,
}

struct Admitted<S>(Arc<Listener<S>>);

impl<S> Drop for Admitted<S> {
    fn drop(&mut self) {
        self.0.gauge.release();
    }
}

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
    gauge: Arc<UdpInflightGauge>,
) -> ListenerDied {
    let listener = Arc::new(Listener { socket, gauge });
    let mut policy = RetryPolicy::new();
    // Max DNS-over-UDP message size (RFC 6891 practical ceiling); anything
    // larger is not a DNS packet.
    let mut buf = [0u8; 65535];
    loop {
        let (len, client) = match listener.socket.recv_from(&mut buf).await {
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
        if !listener.gauge.admit() {
            continue;
        }
        let datagram = buf[..len].to_vec();
        let admitted = Admitted(Arc::clone(&listener));
        let pipeline = Arc::clone(&pipeline);
        tokio::spawn(async move {
            handle_datagram(&admitted.0, &pipeline, &datagram, client).await;
        });
    }
}

async fn handle_datagram<S: Datagrams, F: Forwarder>(
    listener: &Listener<S>,
    pipeline: &Pipeline<F>,
    datagram: &[u8],
    client: SocketAddr,
) {
    let Some(reply) = pipeline.handle(datagram, client.ip(), Transport::Udp).await else {
        return;
    };
    let Err(err) = listener.socket.send_to(&reply, client).await else {
        return;
    };
    if is_client_unreachable(&err) {
        debug!(error = %err, client = %client, "UDP DNS reply undeliverable");
        return;
    }
    match listener.gauge.send_failures.note(std::time::Instant::now()) {
        Some(failures) => {
            warn!(error = %err, client = %client, failures, "failed to send UDP DNS reply")
        }
        None => debug!(error = %err, client = %client, "failed to send UDP DNS reply"),
    }
}

fn is_client_unreachable(err: &io::Error) -> bool {
    use std::io::ErrorKind::{HostUnreachable, NetworkUnreachable, PermissionDenied};
    matches!(
        err.kind(),
        HostUnreachable | NetworkUnreachable | PermissionDenied
    )
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::net::Ipv4Addr;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    use hickory_proto::op::{Message, Query as WireQuery};
    use hickory_proto::rr::{Name, RecordType};
    use tokio::sync::Notify;

    use super::*;
    use crate::testkit;
    use crate::upstream::ForwardOutcome;
    use fah_common::retry::FATAL_CONSECUTIVE_ERRORS;

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

    #[test]
    fn a_refused_port_is_not_classified_because_this_socket_is_never_connected() {
        assert!(
            !is_client_unreachable(&io::Error::from(io::ErrorKind::ConnectionRefused)),
            "not produced by the unconnected send_to this listener uses, in the \
             environment measured by crates/fah-dns/examples/udp_send_errno.rs; \
             the probe saw errno 111 only on a connected socket"
        );
    }

    #[test]
    fn a_network_that_cannot_take_the_reply_is_not_our_fault() {
        for kind in [
            io::ErrorKind::HostUnreachable,
            io::ErrorKind::NetworkUnreachable,
            io::ErrorKind::PermissionDenied,
        ] {
            assert!(
                is_client_unreachable(&io::Error::from(kind)),
                "{kind:?} is the client's network, not a fault of ours"
            );
        }
    }

    #[test]
    fn a_reply_that_does_not_fit_is_our_own_truncation_bug() {
        for kind in [
            io::ErrorKind::InvalidInput,
            io::ErrorKind::Other,
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::UnexpectedEof,
        ] {
            assert!(
                !is_client_unreachable(&io::Error::from(kind)),
                "{kind:?} must keep its warning: the stream kinds never reach a datagram send"
            );
        }
    }

    struct RefusingSocket {
        kind: io::ErrorKind,
    }

    impl Datagrams for RefusingSocket {
        async fn recv_from(&self, _buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
            Err(io::Error::other("this socket exercises the send path only"))
        }

        async fn send_to(&self, _reply: &[u8], _client: SocketAddr) -> io::Result<usize> {
            Err(io::Error::from(self.kind))
        }
    }

    async fn send_failures_after(kind: io::ErrorKind) -> u64 {
        let (pipeline, _data_dir) = testkit::pipeline();
        let gauge = Arc::new(UdpInflightGauge::new(0));
        let listener = Listener {
            socket: RefusingSocket { kind },
            gauge: Arc::clone(&gauge),
        };
        handle_datagram(
            &listener,
            &pipeline,
            &query("wiring.example."),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 5353)),
        )
        .await;
        gauge.send_failures.total()
    }

    #[tokio::test]
    async fn an_undeliverable_reply_never_spends_the_warn_budget() {
        for kind in [
            io::ErrorKind::HostUnreachable,
            io::ErrorKind::NetworkUnreachable,
            io::ErrorKind::PermissionDenied,
        ] {
            assert_eq!(
                send_failures_after(kind).await,
                0,
                "{kind:?} is the client's own network and must not reach the throttle"
            );
        }
    }

    #[tokio::test]
    async fn a_send_fault_of_our_own_reaches_the_throttle() {
        for kind in [
            io::ErrorKind::InvalidInput,
            io::ErrorKind::Other,
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::ConnectionRefused,
        ] {
            assert_eq!(
                send_failures_after(kind).await,
                1,
                "{kind:?} is ours and must be counted and logged"
            );
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

        let died = run(socket, pipeline, Arc::new(UdpInflightGauge::new(0))).await;

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

    #[derive(Clone)]
    struct StallForwarder {
        release: Arc<Notify>,
        calls: Arc<AtomicU32>,
    }

    impl Forwarder for StallForwarder {
        async fn forward(&self, query: &Message) -> io::Result<ForwardOutcome> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.release.notified().await;
            Ok(ForwardOutcome::new(
                Message::response(query.metadata.id, query.metadata.op_code),
                0,
            ))
        }
    }

    struct QueuedSocket {
        datagrams: Mutex<VecDeque<Vec<u8>>>,
        arrived: Notify,
        sent: AtomicU32,
    }

    impl QueuedSocket {
        fn push(&self, datagram: Vec<u8>) {
            self.datagrams.lock().unwrap().push_back(datagram);
            self.arrived.notify_one();
        }
    }

    impl Datagrams for Arc<QueuedSocket> {
        async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
            loop {
                let next = self.datagrams.lock().unwrap().pop_front();
                if let Some(datagram) = next {
                    buf[..datagram.len()].copy_from_slice(&datagram);
                    return Ok((
                        datagram.len(),
                        SocketAddr::from((Ipv4Addr::LOCALHOST, 5353)),
                    ));
                }
                self.arrived.notified().await;
            }
        }

        async fn send_to(&self, reply: &[u8], _client: SocketAddr) -> io::Result<usize> {
            self.sent.fetch_add(1, Ordering::Relaxed);
            Ok(reply.len())
        }
    }

    fn query(name: &str) -> Vec<u8> {
        let mut message = Message::query();
        message.add_query(WireQuery::query(
            Name::from_ascii(name).unwrap(),
            RecordType::A,
        ));
        message.to_vec().unwrap()
    }

    struct Stalled {
        release: Arc<Notify>,
        calls: Arc<AtomicU32>,
        socket: Arc<QueuedSocket>,
        gauge: Arc<UdpInflightGauge>,
        server: tokio::task::JoinHandle<ListenerDied>,
        _data_dir: tempfile::TempDir,
    }

    impl Stalled {
        fn sent(&self) -> u32 {
            self.socket.sent.load(Ordering::Relaxed)
        }

        fn calls(&self) -> u32 {
            self.calls.load(Ordering::Relaxed)
        }
    }

    fn three_queries_against_a_stalled_upstream(max_inflight: usize) -> Stalled {
        let release = Arc::new(Notify::new());
        let calls = Arc::new(AtomicU32::new(0));
        let (pipeline, data_dir) = testkit::pipeline_with(StallForwarder {
            release: Arc::clone(&release),
            calls: Arc::clone(&calls),
        });
        let socket = Arc::new(QueuedSocket {
            datagrams: Mutex::new(VecDeque::from([
                query("q1.example."),
                query("q2.example."),
                query("q3.example."),
            ])),
            arrived: Notify::new(),
            sent: AtomicU32::new(0),
        });
        let gauge = Arc::new(UdpInflightGauge::new(max_inflight));
        let server = tokio::spawn(run(Arc::clone(&socket), pipeline, Arc::clone(&gauge)));
        Stalled {
            release,
            calls,
            socket,
            gauge,
            server,
            _data_dir: data_dir,
        }
    }

    fn inflight(active: u64, peak: u64, shed: u64) -> fah_model::DnsUdpInflight {
        fah_model::DnsUdpInflight { active, peak, shed }
    }

    async fn settle() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn the_inflight_cap_sheds_past_the_limit_and_counts() {
        let stalled = three_queries_against_a_stalled_upstream(1);
        settle().await;

        assert_eq!(
            stalled.calls(),
            1,
            "only the admitted query reaches the forwarder"
        );
        assert_eq!(stalled.gauge.snapshot(), inflight(1, 1, 2));

        stalled.release.notify_one();
        settle().await;
        assert_eq!(stalled.sent(), 1);
        assert_eq!(stalled.gauge.snapshot(), inflight(0, 1, 2));
        stalled.server.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_ceiling_above_one_admits_up_to_it_and_peaks_there() {
        let stalled = three_queries_against_a_stalled_upstream(2);
        settle().await;

        assert_eq!(stalled.calls(), 2, "two slots, two queries in flight");
        assert_eq!(stalled.gauge.snapshot(), inflight(2, 2, 1));

        stalled.release.notify_waiters();
        settle().await;
        assert_eq!(stalled.sent(), 2);
        assert_eq!(stalled.gauge.snapshot(), inflight(0, 2, 1));
        stalled.server.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_released_slot_admits_the_next_datagram() {
        let stalled = three_queries_against_a_stalled_upstream(1);
        settle().await;
        assert_eq!(stalled.gauge.snapshot(), inflight(1, 1, 2));

        stalled.socket.push(query("q4.example."));
        settle().await;
        assert_eq!(stalled.calls(), 1, "a full ceiling sheds the late arrival");
        assert_eq!(stalled.gauge.snapshot(), inflight(1, 1, 3));

        stalled.release.notify_one();
        settle().await;
        assert_eq!(stalled.sent(), 1);
        assert_eq!(stalled.gauge.snapshot(), inflight(0, 1, 3));

        stalled.socket.push(query("q5.example."));
        settle().await;
        assert_eq!(
            stalled.calls(),
            2,
            "the freed slot admits the next datagram"
        );
        assert_eq!(stalled.gauge.snapshot(), inflight(1, 1, 3));

        stalled.release.notify_one();
        settle().await;
        assert_eq!(stalled.sent(), 2);
        assert_eq!(stalled.gauge.snapshot(), inflight(0, 1, 3));
        stalled.server.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn zero_means_no_cap_and_never_sheds() {
        let stalled = three_queries_against_a_stalled_upstream(0);
        settle().await;

        assert_eq!(
            stalled.calls(),
            3,
            "every query is admitted and in flight at once"
        );
        assert_eq!(stalled.gauge.snapshot(), inflight(0, 0, 0));

        stalled.release.notify_waiters();
        settle().await;
        assert_eq!(stalled.sent(), 3);
        assert_eq!(stalled.gauge.snapshot(), inflight(0, 0, 0));
        stalled.server.abort();
    }
}

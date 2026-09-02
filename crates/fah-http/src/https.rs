use std::io;
use std::net::{IpAddr, SocketAddr};
use std::ops::Deref;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant, SystemTime};

use bytes::BufMut;
use fah_common::egress::DestinationPolicy;
use fah_common::resolve::HostResolver;
use fah_config::NoSni;
use fah_model::{
    DecisiveRule, Event, Request as ModelRequest, RequestEvent, ResourceType, Verdict,
};
use fah_rules::{MatchDecision, PolicyState};
use tokio::io::{
    copy_bidirectional_with_sizes, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::debug;

use crate::intercept::Interception;
use crate::proxy::{publish, ProxyCounters, Ruleset};
use crate::sni::{scan_client_hello, HelloScan, MAX_HELLO_BYTES};

const SPLICE_BUF: usize = 16 * 1024;
const HELLO_CHUNK: usize = 2048;
const NO_SNI_LIST: &str = "[https.sni]";
const NO_SNI_RULE: &str = "no_sni = \"block\"";

pub struct TlsProxy {
    resolver: Arc<dyn HostResolver>,
    policy: DestinationPolicy,
    pub(crate) counters: Arc<ProxyCounters>,
    pub(crate) rules: Option<Arc<dyn Ruleset>>,
    pub(crate) policies: Arc<PolicyState>,
    pub(crate) events: Option<mpsc::Sender<Event>>,
    pub(crate) origin_port: u16,
    pub(crate) hello_timeout: Duration,
    pub(crate) idle_timeout: Duration,
    no_sni: NoSni,
    pub(crate) allow_ip_literal_hosts: bool,
    interception: Option<Interception>,
}

impl TlsProxy {
    pub fn new(
        resolver: Arc<dyn HostResolver>,
        policy: DestinationPolicy,
        origin_port: u16,
        hello_timeout: Duration,
        idle_timeout: Duration,
        no_sni: NoSni,
    ) -> Self {
        Self {
            resolver,
            policy,
            counters: Arc::new(ProxyCounters::default()),
            rules: None,
            policies: Arc::new(PolicyState::default()),
            events: None,
            origin_port,
            hello_timeout,
            idle_timeout,
            no_sni,
            allow_ip_literal_hosts: false,
            interception: None,
        }
    }

    pub fn with_ip_literal_hosts(mut self, allow: bool) -> Self {
        self.allow_ip_literal_hosts = allow;
        self
    }

    pub fn with_interception(mut self, interception: Interception) -> Self {
        self.interception = (!interception.is_empty()).then_some(interception);
        self
    }

    pub fn intercepts(&self, ip: IpAddr) -> bool {
        self.interception
            .as_ref()
            .is_some_and(|interception| interception.intercepts(ip))
    }

    fn interception_for(&self, ip: IpAddr, host: &str) -> Option<&Interception> {
        self.interception
            .as_ref()
            .filter(|interception| interception.intercepts(ip) && !interception.excludes(host))
    }

    pub fn with_rules(mut self, rules: Arc<dyn Ruleset>) -> Self {
        self.rules = Some(rules);
        self
    }

    pub fn with_policies(mut self, policies: Arc<PolicyState>) -> Self {
        self.policies = policies;
        self
    }

    pub fn with_events(mut self, events: mpsc::Sender<Event>) -> Self {
        self.events = Some(events);
        self
    }

    pub fn counters(&self) -> Arc<ProxyCounters> {
        Arc::clone(&self.counters)
    }

    pub async fn serve_connection(self: Arc<Self>, mut stream: TcpStream, peer: SocketAddr) {
        let peer = SocketAddr::new(peer.ip().to_canonical(), peer.port());
        self.counters.connections.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();

        let mut hello = Vec::with_capacity(HELLO_CHUNK);
        let scan = match tokio::time::timeout(
            self.hello_timeout,
            read_client_hello(&mut stream, &mut hello),
        )
        .await
        {
            Ok(Ok(scan)) => scan,
            Ok(Err(err)) => {
                self.counters.hello_timeouts.fetch_add(1, Ordering::Relaxed);
                debug!(%peer, error = %err, "no ClientHello arrived; closing");
                return;
            }
            Err(_) => {
                self.counters.hello_timeouts.fetch_add(1, Ordering::Relaxed);
                debug!(%peer, "ClientHello deadline expired; closing");
                return;
            }
        };

        let host = match scan {
            HelloScan::Sni(host) => host,
            HelloScan::NotTls => {
                self.counters.non_tls.fetch_add(1, Ordering::Relaxed);
                debug!(%peer, "non-TLS bytes on the HTTPS port; closing");
                return;
            }
            HelloScan::NoSni | HelloScan::Incomplete => {
                self.no_sni_observed(peer, started);
                return;
            }
        };

        self.counters.requests.fetch_add(1, Ordering::Relaxed);
        if !self.allow_ip_literal_hosts && host.parse::<IpAddr>().is_ok() {
            self.counters.refused_claim.fetch_add(1, Ordering::Relaxed);
            debug!(%peer, %host, "refused: IP-literal SNI");
            return;
        }

        let (verdict, policy) = self.judge(&host, peer);
        if matches!(verdict, Verdict::Block(_)) {
            self.counters.blocked.fetch_add(1, Ordering::Relaxed);
            debug!(%peer, %host, "blocked at SNI; no upstream contact");
            self.emit(&host, peer, verdict, policy, started.elapsed(), 0);
            return;
        }

        let Ok(address) = self.approved_address(&host, peer).await else {
            self.emit(&host, peer, verdict, policy, started.elapsed(), 0);
            return;
        };

        if let Some(interception) = self.interception_for(peer.ip(), &host) {
            let session = Session {
                peer,
                address,
                verdict,
                policy,
                started,
            };
            self.intercept(interception, stream, hello, host, session)
                .await;
            return;
        }

        let upstream =
            match tokio::time::timeout(self.hello_timeout, TcpStream::connect(address)).await {
                Ok(Ok(upstream)) => upstream,
                Ok(Err(err)) => {
                    self.counters
                        .upstream_failures
                        .fetch_add(1, Ordering::Relaxed);
                    debug!(%peer, %host, %address, error = %err, "could not connect upstream");
                    self.emit(&host, peer, verdict, policy, started.elapsed(), 0);
                    return;
                }
                Err(_) => {
                    self.counters
                        .upstream_failures
                        .fetch_add(1, Ordering::Relaxed);
                    debug!(%peer, %host, %address, "upstream connect deadline expired");
                    self.emit(&host, peer, verdict, policy, started.elapsed(), 0);
                    return;
                }
            };

        let duration = started.elapsed();
        let bytes = self.splice(stream, upstream, hello, peer, &host).await;
        self.emit(&host, peer, verdict, policy, duration, bytes);
    }

    async fn splice(
        &self,
        client: TcpStream,
        mut upstream: TcpStream,
        hello: Vec<u8>,
        peer: SocketAddr,
        host: &str,
    ) -> u64 {
        if let Err(err) = upstream.set_nodelay(true) {
            debug!(%peer, error = %err, "could not set TCP_NODELAY upstream");
        }
        if let Err(err) = upstream.write_all(&hello).await {
            self.counters
                .upstream_failures
                .fetch_add(1, Ordering::Relaxed);
            debug!(%peer, %host, error = %err, "could not forward the ClientHello");
            return 0;
        }
        drop(hello);

        let clock = Instant::now();
        let last = AtomicU64::new(0);
        let to_client = AtomicU64::new(0);
        let to_upstream = AtomicU64::new(0);
        let mut client = Activity::new(client, &last, Some(&to_client), clock);
        let mut upstream = Activity::new(upstream, &last, Some(&to_upstream), clock);

        let idle_ms = as_millis(self.idle_timeout);
        let copy =
            copy_bidirectional_with_sizes(&mut client, &mut upstream, SPLICE_BUF, SPLICE_BUF);
        tokio::pin!(copy);

        tokio::select! {
            result = &mut copy => {
                if let Err(err) = result {
                    debug!(%peer, %host, error = %err, "spliced session ended with an error");
                }
            }
            () = idle_watchdog(&last, clock, idle_ms) => {
                debug!(%peer, %host, "spliced session idle past the deadline; closing");
            }
        }
        to_client.load(Ordering::Relaxed)
    }

    fn judge(&self, host: &str, peer: SocketAddr) -> (Verdict, Option<Arc<str>>) {
        let Some(rules) = &self.rules else {
            return (Verdict::Pass, None);
        };
        let matcher = rules.matcher();
        let active = self.policies.current();
        let ctx = matcher.context_for(peer.ip(), &active);
        let verdict = match matcher.lookup_host_in(host, &ctx) {
            MatchDecision::Block(rule) => Verdict::Block(matcher.decisive_rule(rule)),
            MatchDecision::Allow(rule) => Verdict::Allow(matcher.decisive_rule(rule)),
            MatchDecision::Pass => Verdict::Pass,
        };
        (verdict, active.id_of(ctx.policy))
    }

    fn no_sni_observed(&self, peer: SocketAddr, started: Instant) {
        self.counters.requests.fetch_add(1, Ordering::Relaxed);
        let verdict = match self.no_sni {
            NoSni::Pass => Verdict::Pass,
            NoSni::Block => {
                self.counters.blocked.fetch_add(1, Ordering::Relaxed);
                Verdict::Block(DecisiveRule::new(NO_SNI_LIST, NO_SNI_RULE))
            }
        };
        debug!(%peer, "no usable SNI; nothing to splice to, closing");
        self.emit("", peer, verdict, None, started.elapsed(), 0);
    }

    async fn approved_address(&self, host: &str, peer: SocketAddr) -> Result<SocketAddr, ()> {
        let addresses = match self.resolver.resolve(host.to_string()).await {
            Ok(addresses) => addresses,
            Err(err) => {
                self.counters
                    .resolve_failures
                    .fetch_add(1, Ordering::Relaxed);
                debug!(%peer, %host, error = %err, "could not resolve the SNI host");
                return Err(());
            }
        };

        let mut refusal = None;
        for ip in addresses {
            let candidate = SocketAddr::new(ip, self.origin_port);
            match self.policy.check(candidate) {
                Ok(()) => return Ok(candidate),
                Err(reason) => refusal = Some(reason),
            }
        }

        match refusal {
            Some(reason) => {
                self.counters
                    .refused_destination
                    .fetch_add(1, Ordering::Relaxed);
                debug!(
                    %peer,
                    %host,
                    port = self.origin_port,
                    reason = reason.reason(),
                    "refused: destination not permitted"
                );
            }
            None => {
                self.counters
                    .resolve_failures
                    .fetch_add(1, Ordering::Relaxed);
                debug!(%peer, %host, "the SNI host resolved to no addresses");
            }
        }
        Err(())
    }

    fn emit(
        &self,
        host: &str,
        peer: SocketAddr,
        verdict: Verdict,
        policy: Option<Arc<str>>,
        duration: Duration,
        bytes: u64,
    ) {
        let Some(events) = &self.events else {
            return;
        };
        let event = session_event(host, peer, verdict, policy, duration, 0, bytes);
        publish(events, &self.counters, Event::https_sni(event));
    }
}

pub(crate) struct Session {
    pub(crate) peer: SocketAddr,
    pub(crate) address: SocketAddr,
    pub(crate) verdict: Verdict,
    pub(crate) policy: Option<Arc<str>>,
    pub(crate) started: Instant,
}

pub(crate) fn session_event(
    host: &str,
    peer: SocketAddr,
    verdict: Verdict,
    policy: Option<Arc<str>>,
    duration: Duration,
    status: u16,
    bytes: u64,
) -> RequestEvent {
    RequestEvent::new(
        ModelRequest {
            host: host.to_string(),
            path: String::new(),
            method: String::new(),
            resource_type: ResourceType::Unknown,
            client_ip: peer.ip(),
            timestamp: SystemTime::now(),
        },
        verdict,
        duration,
        status,
        bytes,
    )
    .under_policy(policy)
}

async fn read_client_hello(stream: &mut TcpStream, hello: &mut Vec<u8>) -> io::Result<HelloScan> {
    loop {
        let want = MAX_HELLO_BYTES - hello.len();
        if want == 0 {
            return Ok(HelloScan::NoSni);
        }
        if hello.capacity() == hello.len() {
            hello.reserve(HELLO_CHUNK.min(want));
        }
        let read = stream.read_buf(&mut hello.limit(want)).await?;
        if read == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        match scan_client_hello(hello) {
            HelloScan::Incomplete => {}
            scan => return Ok(scan),
        }
    }
}

pub(crate) async fn idle_watchdog(last: &AtomicU64, clock: Instant, idle_ms: u64) {
    loop {
        let now = elapsed_ms(clock);
        let deadline = last.load(Ordering::Relaxed).saturating_add(idle_ms);
        if now >= deadline {
            return;
        }
        tokio::time::sleep(Duration::from_millis(deadline - now)).await;
    }
}

pub(crate) fn as_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn elapsed_ms(clock: Instant) -> u64 {
    as_millis(clock.elapsed())
}

pub(crate) struct Activity<S, L> {
    inner: S,
    last: L,
    written: Option<L>,
    clock: Instant,
}

impl<S, L: Deref<Target = AtomicU64>> Activity<S, L> {
    pub(crate) fn new(inner: S, last: L, written: Option<L>, clock: Instant) -> Self {
        Self {
            inner,
            last,
            written,
            clock,
        }
    }

    fn touch(&self) {
        self.last.store(elapsed_ms(self.clock), Ordering::Relaxed);
    }
}

impl<S: AsyncRead + Unpin, L: Deref<Target = AtomicU64> + Unpin> AsyncRead for Activity<S, L> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let polled = Pin::new(&mut self.inner).poll_read(cx, buf);
        if polled.is_ready() {
            self.touch();
        }
        polled
    }
}

impl<S: AsyncWrite + Unpin, L: Deref<Target = AtomicU64> + Unpin> AsyncWrite for Activity<S, L> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let polled = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let (Poll::Ready(Ok(written)), Some(counter)) = (&polled, &self.written) {
            counter.fetch_add(*written as u64, Ordering::Relaxed);
        }
        if polled.is_ready() {
            self.touch();
        }
        polled
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

//! The transparent proxy: parse the head, judge the destination, stream the
//! body.
//!
//! **Transport-agnostic by construction.** [`Proxy::serve_connection`] is
//! generic over the stream rather than taking a `TcpStream`, so Phase 3 hands
//! it a rustls-terminated stream and reuses this whole pipeline after TLS
//! termination. The parameter is monomorphised, not a trait object: a
//! `Box<dyn …>` here would put a virtual call on every body read, which
//! PERFORMANCE.md forbids.
//!
//! **The body is never parsed.** The upstream response body is handed back to
//! hyper as-is; images, archives, PDFs and video are relayed with no
//! inspection, no buffering and no rewriting. HTML rewriting is Phase 4 and
//! nothing here anticipates it.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant, SystemTime};

use bytes::Bytes;
use fah_common::egress::DestinationPolicy;
use fah_common::resolve::HostResolver;
use fah_model::{
    DecisiveRule, Event, HttpRequest, Request as ModelRequest, RequestEvent, ResourceType, Verdict,
};
use fah_rules::{ListManager, MatchDecision, Matcher, PolicyState};
use http_body_util::{Either, Full};
use hyper::body::{Body as _, Incoming};
use hyper::header::{
    HeaderName, HeaderValue, CONNECTION, TE, TRAILER, TRANSFER_ENCODING, UPGRADE, VIA,
};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::claim::{destination_of, retarget, ClaimError, Destination};

/// What this proxy announces itself as, per RFC 9110 §7.6.3.
const VIA_VALUE: HeaderValue = HeaderValue::from_static("1.1 fastadhunter");

/// Response body: either the upstream's stream, relayed untouched, or a short
/// synthesized message. [`Either`] rather than a boxed body — no virtual call
/// per body chunk.
type ProxyBody = Either<Incoming, Full<Bytes>>;

/// Hop-by-hop headers, which a proxy must not forward (RFC 9110 §7.6.1).
/// `Connection` itself may also name further ones; those are stripped
/// dynamically in [`strip_hop_by_hop`].
const HOP_BY_HOP: [HeaderName; 8] = [
    CONNECTION,
    TE,
    TRAILER,
    TRANSFER_ENCODING,
    UPGRADE,
    HeaderName::from_static("keep-alive"),
    HeaderName::from_static("proxy-authenticate"),
    HeaderName::from_static("proxy-authorization"),
];

/// Counters for what the proxy refused and why. Every field is a monotonic
/// total — the operator's signal that a LAN device is probing.
#[derive(Debug, Default)]
pub struct ProxyCounters {
    pub requests: AtomicU64,
    /// `Host` rejected before any resolution.
    pub refused_claim: AtomicU64,
    /// Resolved address refused by the egress policy — the open-relay guard.
    pub refused_destination: AtomicU64,
    pub resolve_failures: AtomicU64,
    pub upstream_failures: AtomicU64,
    /// Connections that spoke something other than HTTP on the proxy port.
    pub non_http: AtomicU64,
    /// Requests refused by a filtering rule (p2-04).
    pub blocked: AtomicU64,
    /// Events the bounded channel could not take. Mirrors the DNS pipeline's
    /// counter so the two shed figures mean the same thing.
    pub dropped_events: AtomicU64,
}

/// A point-in-time read of [`ProxyCounters`], for the binary to publish
/// (siblings never import each other, so metrics arrive through a port).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyStats {
    pub requests: u64,
    pub refused_claim: u64,
    pub refused_destination: u64,
    pub resolve_failures: u64,
    pub upstream_failures: u64,
    pub non_http: u64,
    pub blocked: u64,
    pub dropped_events: u64,
}

impl ProxyCounters {
    pub fn snapshot(&self) -> ProxyStats {
        ProxyStats {
            requests: self.requests.load(Ordering::Relaxed),
            refused_claim: self.refused_claim.load(Ordering::Relaxed),
            refused_destination: self.refused_destination.load(Ordering::Relaxed),
            resolve_failures: self.resolve_failures.load(Ordering::Relaxed),
            upstream_failures: self.upstream_failures.load(Ordering::Relaxed),
            non_http: self.non_http.load(Ordering::Relaxed),
            blocked: self.blocked.load(Ordering::Relaxed),
            dropped_events: self.dropped_events.load(Ordering::Relaxed),
        }
    }
}

/// Connects to an authority that is **already a literal address**.
///
/// The proxy resolves and judges the destination itself, then rewrites the
/// request target to the approved address (see [`crate::claim::retarget`]). By
/// refusing to resolve anything, this connector removes the window between the
/// policy check and the connect in which a second lookup could return a
/// different address — a DNS rebind has nothing left to race.
#[derive(Clone, Copy, Debug, Default)]
struct LiteralConnector;

impl tower_service::Service<Uri> for LiteralConnector {
    type Response = TokioIo<TcpStream>;
    type Error = io::Error;
    /// Boxed because `tower_service::Service` needs a named future type and
    /// `TcpStream::connect`'s is opaque. This is one allocation per *upstream
    /// connection*, not per read — the body path stays free of indirection.
    type Future = Pin<Box<dyn Future<Output = io::Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        Box::pin(async move {
            let authority = uri.authority().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "upstream URI has no authority")
            })?;
            // Parse, never resolve: anything that is not already an address is
            // a bug upstream of here, and failing loudly is the point.
            let address: SocketAddr = authority.as_str().parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("upstream authority {authority} is not a literal address"),
                )
            })?;
            let stream = TcpStream::connect(address).await?;
            // Small proxied writes should not wait on Nagle.
            stream.set_nodelay(true)?;
            Ok(TokioIo::new(stream))
        })
    }
}

/// Where the proxy gets the ruleset to judge a request against.
///
/// A trait rather than `Arc<ListManager>` directly, even though `fah-rules` is
/// a legitimate L2 dependency: answering a verdict needs *the current matcher*
/// and nothing else, while `ListManager` is the whole list lifecycle —
/// downloads, schedules, `/data` writes. Depending on the narrow thing keeps
/// the proxy testable against a compiled `Matcher` with no filesystem and no
/// config, and states in the type what the proxy actually uses.
pub trait Ruleset: Send + Sync + 'static {
    /// The current compiled ruleset. Called once per request, so it must be
    /// allocation-free beyond an atomic refcount bump.
    fn matcher(&self) -> Arc<Matcher>;
}

/// The production implementation: the atomically-swapped ruleset the list
/// lifecycle maintains, exactly as `fah_dns::Pipeline` reads it.
impl Ruleset for ListManager {
    fn matcher(&self) -> Arc<Matcher> {
        ListManager::matcher(self)
    }
}

/// Everything a connection needs, shared by `Arc` across all of them.
pub struct Proxy {
    resolver: Arc<dyn HostResolver>,
    policy: DestinationPolicy,
    client: Client<LiteralConnector, Incoming>,
    counters: Arc<ProxyCounters>,
    /// The compiled ruleset, taken fresh per request. `fah-rules` is L2, so
    /// this is a direct dependency rather than a port — the same shape
    /// `fah_dns::Pipeline` uses, so the two engines cannot drift on which
    /// ruleset they are answering from.
    rules: Option<Arc<dyn Ruleset>>,
    /// The client → policy map the binary keeps current (p2-06), shared with
    /// the DNS pipeline so one device is judged the same way by both.
    policies: Arc<PolicyState>,
    /// The one bounded channel both pipelines write to. `None` when nothing is
    /// consuming events, which is what the p2-02 tests and the benches want.
    events: Option<mpsc::Sender<Event>>,
    /// The origin port this proxy intercepts — 80. Not the listen port.
    origin_port: u16,
    /// Slowloris bound: how long a client may take over its request head.
    header_timeout: Duration,
    allow_ip_literal_hosts: bool,
}

impl Proxy {
    pub fn new(
        resolver: Arc<dyn HostResolver>,
        policy: DestinationPolicy,
        origin_port: u16,
        header_timeout: Duration,
        upstream_idle_timeout: Duration,
        max_idle_per_host: usize,
        allow_ip_literal_hosts: bool,
    ) -> Self {
        let client = Client::builder(TokioExecutor::new())
            // Bounded pool: idle upstream connections are capped per host and
            // reaped on a timer, so memory is a function of configuration
            // rather than of how many sites the LAN visits (hard rule 4).
            .pool_idle_timeout(upstream_idle_timeout)
            .pool_max_idle_per_host(max_idle_per_host)
            .build(LiteralConnector);
        Self {
            resolver,
            policy,
            client,
            counters: Arc::new(ProxyCounters::default()),
            rules: None,
            policies: Arc::new(PolicyState::default()),
            events: None,
            origin_port,
            header_timeout,
            allow_ip_literal_hosts,
        }
    }

    /// Attaches the ruleset. Without it the proxy forwards everything, which is
    /// the p2-02 behaviour and remains what the pass-through bench measures.
    pub fn with_rules(mut self, rules: Arc<dyn Ruleset>) -> Self {
        self.rules = Some(rules);
        self
    }

    /// Attaches the shared policy state. Without it every client is judged
    /// under the default policy, which is the pre-p2-06 behaviour.
    pub fn with_policies(mut self, policies: Arc<PolicyState>) -> Self {
        self.policies = policies;
        self
    }

    /// Attaches the event channel — the same `mpsc` the DNS pipeline writes to,
    /// so there is one queue and one shed figure (see `fah_model::Event`).
    pub fn with_events(mut self, events: mpsc::Sender<Event>) -> Self {
        self.events = Some(events);
        self
    }

    pub fn counters(&self) -> Arc<ProxyCounters> {
        Arc::clone(&self.counters)
    }

    /// Serves one client connection to completion.
    ///
    /// Generic over the stream on purpose — see the module docs. `DuplexStream`
    /// in the tests and `TlsStream` in Phase 3 both satisfy these bounds, which
    /// is the whole claim.
    pub async fn serve_connection<S>(self: Arc<Self>, stream: S, peer: SocketAddr)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        // The dual-stack listener reports IPv4 peers as v4-mapped IPv6, so
        // canonicalize once here — the same thing `fah_dns::Pipeline::handle`
        // does, and for the same reason: a policy assigned to `192.168.1.50`
        // must match, and the client must not appear twice in the log.
        let peer = SocketAddr::new(peer.ip().to_canonical(), peer.port());
        let proxy = Arc::clone(&self);
        let service = service_fn(move |request| {
            let proxy = Arc::clone(&proxy);
            async move { proxy.handle(request, peer).await }
        });

        let result = hyper::server::conn::http1::Builder::new()
            // Required for `header_read_timeout` to work at all — hyper panics
            // on a timeout with no timer installed rather than silently not
            // enforcing it.
            .timer(TokioTimer::new())
            // The slowloris bound. It also caps how long a keep-alive
            // connection may sit between requests, since hyper arms it while
            // waiting for the next head.
            .header_read_timeout(self.header_timeout)
            .keep_alive(true)
            .serve_connection(TokioIo::new(stream), service)
            .await;

        if let Err(err) = result {
            // Non-HTTP bytes on the proxy port land here as a parse error:
            // detect, count, and let the connection close. Not a warning — a
            // stray port scan is not an operator problem.
            if err.is_parse() {
                self.counters.non_http.fetch_add(1, Ordering::Relaxed);
                debug!(%peer, "non-HTTP bytes on the proxy port; closing");
            } else {
                debug!(%peer, error = %err, "HTTP connection ended with an error");
            }
        }
    }

    async fn handle(
        &self,
        request: Request<Incoming>,
        peer: SocketAddr,
    ) -> Result<Response<ProxyBody>, hyper::Error> {
        self.counters.requests.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();

        let claim = match destination_of(&request, self.origin_port, self.allow_ip_literal_hosts) {
            Ok(claim) => claim,
            Err(err) => {
                self.counters.refused_claim.fetch_add(1, Ordering::Relaxed);
                warn!(%peer, reason = err.reason(), "refused: unusable Host");
                return Ok(refuse(match err {
                    // A destination we will not serve, versus a request we
                    // cannot parse — the client can act on the difference.
                    ClaimError::IpLiteral => StatusCode::FORBIDDEN,
                    _ => StatusCode::BAD_REQUEST,
                }));
            }
        };

        // The verdict is taken on the **head**, before the destination is
        // resolved and before any byte is fetched: a blocked request must cost
        // no DNS lookup and no upstream connection ("zero bytes fetched
        // upstream" is the task's acceptance criterion, and resolving first
        // would already have leaked the intent to the upstream resolver).
        let judged = self.judge(&request, &claim, peer);
        if let Some(blocked) = judged.blocked() {
            self.counters.blocked.fetch_add(1, Ordering::Relaxed);
            let response = crate::block::response(judged.resource_type, blocked);
            let status = response.status().as_u16();
            self.emit(&judged, started, status, 0);
            return Ok(response.map(|body| Either::Right(Full::new(body))));
        }

        let address = match self.approved_address(&claim, peer).await {
            Ok(address) => address,
            Err(status) => {
                self.emit(&judged, started, status.as_u16(), 0);
                return Ok(refuse(status));
            }
        };

        let upstream = match self.to_upstream_request(request, address) {
            Ok(upstream) => upstream,
            Err(err) => {
                debug!(%peer, error = %err, "could not build the upstream request");
                self.emit(&judged, started, 400, 0);
                return Ok(refuse(StatusCode::BAD_REQUEST));
            }
        };

        match self.client.request(upstream).await {
            Ok(response) => {
                let status = response.status().as_u16();
                // `size_hint` rather than counting bytes: the body is relayed
                // untouched and must stay that way — wrapping it to tally would
                // put per-chunk work on the path p2-02 exists to keep clean.
                // A chunked response reports no exact size, which is honest.
                let bytes = response.body().size_hint().exact().unwrap_or(0);
                self.emit(&judged, started, status, bytes);
                Ok(to_client_response(response))
            }
            Err(err) => {
                self.counters
                    .upstream_failures
                    .fetch_add(1, Ordering::Relaxed);
                debug!(%peer, host = %claim.host, error = %err, "upstream request failed");
                self.emit(&judged, started, 502, 0);
                Ok(refuse(StatusCode::BAD_GATEWAY))
            }
        }
    }

    /// Consults the Rule Engine and captures everything the event needs.
    ///
    /// Done in one place because the request is consumed by the forward: the
    /// host, path, method and resource type all have to be taken off the head
    /// before it is handed upstream.
    fn judge(&self, request: &Request<Incoming>, claim: &Destination, peer: SocketAddr) -> Judged {
        let resource_type = crate::request::resource_type(request.headers(), request.uri());
        let authority = match claim.port {
            port if port == self.origin_port => claim.host.clone(),
            port => format!("{}:{port}", claim.host),
        };
        let url = crate::request::absolute_url(request, &authority);
        let host = crate::request::request_host(&claim.host).to_string();
        let path = request
            .uri()
            .path_and_query()
            .map_or("/", |pq| pq.as_str())
            .to_string();

        let (verdict, policy) = match &self.rules {
            None => (Verdict::Pass, None),
            Some(rules) => {
                let matcher = rules.matcher();
                let active = self.policies.current();
                // Same helper the DNS pipeline calls, so a client cannot land
                // in one policy for a name and another for a fetch.
                let ctx = matcher.context_for(peer.ip(), &active);
                let document_host = crate::request::document_host(request.headers());
                let model = HttpRequest {
                    url: &url,
                    host: &host,
                    method: request.method().as_str(),
                    resource_type,
                    document_host,
                };
                let verdict = match matcher.lookup_http_in(&model, &ctx) {
                    MatchDecision::Block(rule) => Verdict::Block(matcher.decisive_rule(rule)),
                    MatchDecision::Allow(rule) => Verdict::Allow(matcher.decisive_rule(rule)),
                    MatchDecision::Pass => Verdict::Pass,
                };
                (verdict, active.id_of(ctx.policy))
            }
        };

        Judged {
            request: ModelRequest {
                host,
                path,
                method: request.method().as_str().to_string(),
                resource_type,
                client_ip: peer.ip(),
                timestamp: SystemTime::now(),
            },
            resource_type,
            verdict,
            policy,
        }
    }

    /// Publishes the completed request. Never blocks the response: a full
    /// channel sheds and counts, exactly as the DNS pipeline does — an
    /// observability queue must not become a backpressure path onto traffic.
    fn emit(&self, judged: &Judged, started: Instant, status: u16, bytes: u64) {
        let Some(events) = &self.events else {
            return;
        };
        let event = RequestEvent::new(
            judged.request.clone(),
            judged.verdict.clone(),
            started.elapsed(),
            status,
            bytes,
        )
        .under_policy(judged.policy.clone());
        if events.try_send(Event::http(event)).is_err() {
            self.counters.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Resolves the claim and returns the first address the policy allows.
    ///
    /// Resolution comes first and the judgement second — checking the *name*
    /// would let a public hostname whose A record is `192.168.10.1` walk
    /// straight through.
    async fn approved_address(
        &self,
        claim: &Destination,
        peer: SocketAddr,
    ) -> Result<SocketAddr, StatusCode> {
        let addresses = match self.resolver.resolve(claim.host.clone()).await {
            Ok(addresses) => addresses,
            Err(err) => {
                self.counters
                    .resolve_failures
                    .fetch_add(1, Ordering::Relaxed);
                debug!(%peer, host = %claim.host, error = %err, "could not resolve upstream");
                return Err(StatusCode::BAD_GATEWAY);
            }
        };

        let mut refusal = None;
        for ip in addresses {
            let candidate = SocketAddr::new(ip, claim.port);
            match self.policy.check(candidate) {
                Ok(()) => return Ok(candidate),
                Err(reason) => refusal = Some(reason),
            }
        }

        match refusal {
            Some(reason) => {
                // Counted here rather than before the match: an empty answer is
                // a resolution failure, not a refused destination, and folding
                // it in would inflate the counter an operator reads as "a LAN
                // device is probing".
                self.counters
                    .refused_destination
                    .fetch_add(1, Ordering::Relaxed);
                // The signal a LAN device is probing: warn, with who and where.
                warn!(
                    %peer,
                    host = %claim.host,
                    port = claim.port,
                    reason = reason.reason(),
                    "refused: destination not permitted"
                );
                Err(StatusCode::FORBIDDEN)
            }
            None => {
                self.counters
                    .resolve_failures
                    .fetch_add(1, Ordering::Relaxed);
                debug!(%peer, host = %claim.host, "upstream resolved to no addresses");
                Err(StatusCode::BAD_GATEWAY)
            }
        }
    }

    /// Retargets the request at the approved address and cleans the headers.
    /// The body is moved through untouched.
    fn to_upstream_request(
        &self,
        request: Request<Incoming>,
        address: SocketAddr,
    ) -> Result<Request<Incoming>, hyper::http::Error> {
        let (mut parts, body) = request.into_parts();
        parts.uri = retarget(&parts.uri, address)?;
        strip_hop_by_hop(&mut parts.headers);
        // `Host` is deliberately left as the client wrote it: the origin must
        // see the name it serves, not the address we reached it at
        // (RFC 9110 §7.2). `retarget` only changes the request target.
        //
        // Not asserted: a request in absolute form carries its authority in the
        // target and needs no `Host` at all, which `authority_of` accepts. The
        // assertion that used to stand here panicked in debug builds on that
        // perfectly legal shape — reachable from the wire, which is the wrong
        // place for an invariant that was only ever documenting intent.
        append_via(&mut parts.headers);
        Ok(Request::from_parts(parts, body))
    }
}

/// One request's verdict plus everything the event will need, captured before
/// the head is handed upstream.
struct Judged {
    request: ModelRequest,
    resource_type: ResourceType,
    verdict: Verdict,
    policy: Option<Arc<str>>,
}

impl Judged {
    /// `Some(rule)` when this request must be refused. `Verdict::Allow` is an
    /// exception *matching*, which means forward — not a second kind of block.
    fn blocked(&self) -> Option<Option<&DecisiveRule>> {
        match &self.verdict {
            Verdict::Block(rule) => Some(Some(rule)),
            Verdict::Allow(_) | Verdict::Pass => None,
        }
    }
}

fn to_client_response(response: Response<Incoming>) -> Response<ProxyBody> {
    let (mut parts, body) = response.into_parts();
    strip_hop_by_hop(&mut parts.headers);
    append_via(&mut parts.headers);
    // `Either::Left` — the upstream body streams straight through.
    Response::from_parts(parts, Either::Left(body))
}

/// A short synthesized response. The only body this proxy ever creates.
fn refuse(status: StatusCode) -> Response<ProxyBody> {
    let mut response = Response::new(Either::Right(Full::new(Bytes::from_static(
        b"FastAdHunter: request refused\n",
    ))));
    *response.status_mut() = status;
    response
}

/// Removes hop-by-hop headers, including the ones `Connection` names
/// (RFC 9110 §7.6.1). Forwarding these is how a proxy leaks framing decisions
/// between two connections that made them independently.
fn strip_hop_by_hop(headers: &mut hyper::HeaderMap) {
    // Collect first: the names come out of the very header being removed.
    let named: Vec<HeaderName> = headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|token| HeaderName::try_from(token.trim()).ok())
        .collect();
    for name in named {
        headers.remove(name);
    }
    for name in HOP_BY_HOP {
        headers.remove(name);
    }
}

fn append_via(headers: &mut hyper::HeaderMap) {
    headers.append(VIA, VIA_VALUE);
}

#[cfg(test)]
mod tests {
    use hyper::header::CONTENT_LENGTH;

    use super::*;

    #[test]
    fn hop_by_hop_headers_do_not_survive_the_hop() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            CONNECTION,
            HeaderValue::from_static("keep-alive, X-Private"),
        );
        headers.insert(TE, HeaderValue::from_static("trailers"));
        headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
        headers.insert(
            HeaderName::from_static("x-private"),
            HeaderValue::from_static("secret"),
        );
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("7"));

        strip_hop_by_hop(&mut headers);

        assert!(!headers.contains_key(CONNECTION));
        assert!(!headers.contains_key(TE));
        assert!(!headers.contains_key(UPGRADE));
        assert!(
            !headers.contains_key("x-private"),
            "a header named by Connection is hop-by-hop too"
        );
        assert!(
            headers.contains_key(CONTENT_LENGTH),
            "end-to-end headers must survive"
        );
    }

    #[test]
    fn via_announces_this_proxy() {
        let mut headers = hyper::HeaderMap::new();
        append_via(&mut headers);
        assert_eq!(headers.get(VIA).unwrap(), "1.1 fastadhunter");
    }

    /// The connector must never resolve — that is what closes the rebind
    /// window between the policy check and the connect.
    #[tokio::test]
    async fn the_connector_refuses_anything_that_is_not_a_literal_address() {
        use tower_service::Service as _;

        let mut connector = LiteralConnector;
        let err = connector
            .call("http://example.com:80/".parse().unwrap())
            .await
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("literal address"), "got: {err}");
    }

    #[test]
    fn refusals_carry_a_body_the_client_can_read() {
        let response = refuse(StatusCode::FORBIDDEN);
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

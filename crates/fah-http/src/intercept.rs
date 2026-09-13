use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fah_certs::CertStore;
use fah_model::{Event, CLIENT_CERT_REJECTED, UPSTREAM_CERT_FAILURE};
use fah_rules::interception::InterceptionState;
use http_body_util::{Either, Full};
use hyper::body::{Body as _, Incoming};
use hyper::client::conn::{http1, http2};
use hyper::header::{HeaderValue, HOST};
use hyper::http::uri::{Authority, PathAndQuery, Scheme};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, Uri, Version};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto;
use rustls::{AlertDescription, ClientConfig, ServerConfig};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsAcceptor;
use tracing::debug;

use crate::claim::{destination_of, ClaimError};
use crate::https::{as_millis, idle_watchdog, session_event, Activity, Session, TlsProxy};
use crate::proxy::{
    append_via, emit, judge, publish, refuse, strip_hop_by_hop, to_client_response, Judged,
    ProxyBody, ProxyCounters,
};
use crate::tls::{
    certificate_error, client_alert, connect_verified_upstream, negotiated, Alpn, RewindStream,
};

const SCHEME: &str = "https";
const H2_STREAM_WINDOW: u32 = 64 * 1024;
const H2_MAX_STREAMS: u32 = 64;
const H2_CONNECTION_WINDOW: u32 = H2_MAX_STREAMS * H2_STREAM_WINDOW;
const H2_SEND_BUF: usize = 64 * 1024;
const H1_MAX_BUF: usize = 128 * 1024;

fn upstream_cert_failure_status() -> StatusCode {
    StatusCode::from_u16(UPSTREAM_CERT_FAILURE).unwrap_or(StatusCode::BAD_GATEWAY)
}

fn rejection_status(alert: AlertDescription) -> Option<u16> {
    match alert {
        AlertDescription::BadCertificate
        | AlertDescription::CertificateUnknown
        | AlertDescription::AccessDenied => Some(CLIENT_CERT_REJECTED),
        _ => None,
    }
}

fn alert_counter(counters: &ProxyCounters, alert: AlertDescription) -> Option<&AtomicU64> {
    match alert {
        AlertDescription::BadCertificate => Some(&counters.alert_bad_certificate),
        AlertDescription::CertificateUnknown => Some(&counters.alert_certificate_unknown),
        AlertDescription::AccessDenied => Some(&counters.alert_access_denied),
        _ => None,
    }
}

pub struct Interception {
    server_config: Arc<ServerConfig>,
    client_config: Arc<ClientConfig>,
    store: Arc<CertStore>,
    state: Arc<InterceptionState>,
}

impl Interception {
    pub fn new(
        server_config: Arc<ServerConfig>,
        client_config: Arc<ClientConfig>,
        store: Arc<CertStore>,
        state: Arc<InterceptionState>,
    ) -> Self {
        Self {
            server_config,
            client_config,
            store,
            state,
        }
    }

    pub fn state(&self) -> &Arc<InterceptionState> {
        &self.state
    }
}

impl TlsProxy {
    pub(crate) async fn intercept(
        self: &Arc<Self>,
        interception: &Interception,
        stream: TcpStream,
        hello: Vec<u8>,
        host: Box<str>,
        session: Session,
    ) {
        let peer = session.peer;
        let Some((authority, host_header)) = forward_authority(&host) else {
            debug!(%peer, %host, "the SNI is not a usable request authority; closing");
            self.emit_session(&host, &session, 0);
            return;
        };
        let upstream = match connect_verified_upstream(
            &interception.client_config,
            &host,
            session.address,
            self.hello_timeout,
        )
        .await
        {
            Ok(upstream) => upstream,
            Err(err) if certificate_error(&err) => {
                self.counters
                    .upstream_cert_failures
                    .fetch_add(1, Ordering::Relaxed);
                debug!(%peer, %host, address = %session.address, error = %err, "upstream certificate not verified; closing before our handshake");
                self.emit_session(&host, &session, UPSTREAM_CERT_FAILURE);
                return;
            }
            Err(err) => {
                self.counters
                    .upstream_failures
                    .fetch_add(1, Ordering::Relaxed);
                debug!(%peer, %host, address = %session.address, error = %err, "could not connect upstream; closing before our handshake");
                self.emit_session(&host, &session, 0);
                return;
            }
        };

        let store = Arc::clone(&interception.store);
        let name = host.to_string();
        match tokio::task::spawn_blocking(move || store.prewarm(&name)).await {
            Ok(Ok(_leaf)) => {}
            Ok(Err(err)) => {
                debug!(%peer, %host, error = %err, "no leaf for the host; closing before our handshake");
                self.emit_session(&host, &session, 0);
                return;
            }
            Err(err) => {
                debug!(%peer, %host, error = %err, "leaf minting did not complete; closing");
                self.emit_session(&host, &session, 0);
                return;
            }
        }

        let clock = Instant::now();
        let last = Arc::new(AtomicU64::new(0));
        let metered = Activity::new(stream, Arc::clone(&last), None, clock);
        let acceptor = TlsAcceptor::from(Arc::clone(&interception.server_config));
        let accept = acceptor.accept(RewindStream::new(hello, metered));
        let tls = match tokio::time::timeout(self.hello_timeout, accept).await {
            Ok(Ok(tls)) => {
                self.counters
                    .handshakes_completed
                    .fetch_add(1, Ordering::Relaxed);
                tls
            }
            Ok(Err(err)) => {
                let alert = client_alert(&err);
                match alert.and_then(|alert| rejection_status(alert).map(|status| (alert, status)))
                {
                    Some((alert, status)) => {
                        self.counters
                            .client_cert_rejections
                            .fetch_add(1, Ordering::Relaxed);
                        if let Some(counter) = alert_counter(&self.counters, alert) {
                            counter.fetch_add(1, Ordering::Relaxed);
                        }
                        debug!(%peer, %host, ?alert, "the client rejected our certificate");
                        self.emit_session(&host, &session, status);
                    }
                    None => {
                        match alert {
                            Some(alert) => {
                                debug!(%peer, %host, ?alert, "the client sent a fatal alert outside the rejection set")
                            }
                            None => {
                                debug!(%peer, %host, error = %err, "the client did not complete our handshake")
                            }
                        }
                        self.emit_session(&host, &session, 0);
                    }
                }
                return;
            }
            Err(_) => {
                debug!(%peer, %host, "our handshake deadline expired; closing");
                self.emit_session(&host, &session, 0);
                return;
            }
        };

        let sender = match Sender::handshake(upstream).await {
            Ok(sender) => sender,
            Err(err) => {
                self.counters
                    .upstream_failures
                    .fetch_add(1, Ordering::Relaxed);
                debug!(%peer, %host, error = %err, "upstream HTTP handshake failed");
                self.emit_session(&host, &session, 0);
                return;
            }
        };
        let host: Arc<str> = Arc::from(host);
        let upstream = Arc::new(Upstream {
            config: Arc::clone(&interception.client_config),
            host: Arc::clone(&host),
            authority,
            host_header,
            address: session.address,
            timeout: self.hello_timeout,
            sender: Mutex::new(Some(sender)),
        });

        let proxy = Arc::clone(self);
        let service_host = Arc::clone(&host);
        let service = service_fn(move |request| {
            let proxy = Arc::clone(&proxy);
            let upstream = Arc::clone(&upstream);
            let host = Arc::clone(&service_host);
            async move {
                proxy
                    .handle_intercepted(&upstream, &host, request, peer)
                    .await
            }
        });

        let mut builder = auto::Builder::new(TokioExecutor::new());
        builder
            .http1()
            .timer(TokioTimer::new())
            .header_read_timeout(self.idle_timeout)
            .keep_alive(true)
            .max_buf_size(H1_MAX_BUF);
        builder
            .http2()
            .timer(TokioTimer::new())
            .initial_stream_window_size(H2_STREAM_WINDOW)
            .initial_connection_window_size(H2_CONNECTION_WINDOW)
            .max_concurrent_streams(H2_MAX_STREAMS)
            .max_send_buf_size(H2_SEND_BUF);
        let serving = builder.serve_connection(TokioIo::new(tls), service);
        tokio::pin!(serving);

        let idle_ms = as_millis(self.idle_timeout);
        tokio::select! {
            result = &mut serving => {
                if let Err(err) = result {
                    debug!(%peer, %host, error = %err, "intercepted session ended with an error");
                }
            }
            () = idle_watchdog(&last, clock, idle_ms) => {
                debug!(%peer, %host, "intercepted session idle past the deadline; closing");
            }
        }
    }

    fn emit_session(&self, host: &str, session: &Session, status: u16) {
        let Some(events) = &self.events else {
            return;
        };
        let event = session_event(
            host,
            session.peer,
            session.verdict.clone(),
            session.policy.clone(),
            session.started.elapsed(),
            status,
            0,
        );
        publish(events, &self.counters, Event::https(event));
    }

    fn emit_request(&self, judged: Judged, started: Instant, status: u16, bytes: u64) {
        emit(
            self.events.as_ref(),
            &self.counters,
            Event::https,
            judged,
            started,
            status,
            bytes,
        );
    }

    async fn handle_intercepted(
        &self,
        upstream: &Upstream,
        host: &str,
        request: Request<Incoming>,
        peer: SocketAddr,
    ) -> Result<Response<ProxyBody>, hyper::Error> {
        let started = Instant::now();
        self.counters.requests.fetch_add(1, Ordering::Relaxed);

        let claim = match destination_of(&request, self.origin_port, self.allow_ip_literal_hosts) {
            Ok(claim) => claim,
            Err(err) => {
                self.counters.refused_claim.fetch_add(1, Ordering::Relaxed);
                debug!(%peer, %host, reason = err.reason(), "refused: unusable Host inside TLS");
                return Ok(refuse(match err {
                    ClaimError::IpLiteral => StatusCode::FORBIDDEN,
                    _ => StatusCode::BAD_REQUEST,
                }));
            }
        };

        let judged = judge(
            self.rules.as_deref(),
            &self.policies,
            SCHEME,
            self.origin_port,
            &request,
            &claim,
            peer,
        );
        if let Some(blocked) = judged.blocked() {
            self.counters.blocked.fetch_add(1, Ordering::Relaxed);
            let response = crate::block::response(judged.resource_type, blocked);
            let status = response.status().as_u16();
            self.emit_request(judged, started, status, 0);
            return Ok(response.map(|body| Either::Right(Full::new(body))));
        }

        if !same_host(&claim.host, host) || claim.port != self.origin_port {
            self.counters.refused_claim.fetch_add(1, Ordering::Relaxed);
            debug!(%peer, %host, claimed = %claim.host, port = claim.port, "refused: Host does not name the verified SNI and origin port");
            self.emit_request(judged, started, 421, 0);
            return Ok(refuse(StatusCode::MISDIRECTED_REQUEST));
        }

        match upstream.send(request).await {
            Ok(response) => {
                let status = response.status().as_u16();
                let bytes = response.body().size_hint().exact().unwrap_or(0);
                self.emit_request(judged, started, status, bytes);
                Ok(to_client_response(response))
            }
            Err(err) if certificate_error(&err) => {
                self.counters
                    .upstream_cert_failures
                    .fetch_add(1, Ordering::Relaxed);
                debug!(%peer, %host, error = %err, "upstream certificate not verified on reconnect");
                self.emit_request(judged, started, UPSTREAM_CERT_FAILURE, 0);
                Ok(refuse(upstream_cert_failure_status()))
            }
            Err(err) => {
                self.counters
                    .upstream_failures
                    .fetch_add(1, Ordering::Relaxed);
                debug!(%peer, %host, error = %err, "intercepted upstream request failed");
                self.emit_request(judged, started, 502, 0);
                Ok(refuse(StatusCode::BAD_GATEWAY))
            }
        }
    }
}

fn same_host(claimed: &str, verified: &str) -> bool {
    claimed
        .trim_end_matches('.')
        .eq_ignore_ascii_case(verified.trim_end_matches('.'))
}

fn forward_authority(host: &str) -> Option<(Authority, HeaderValue)> {
    let authority: Authority = host.parse().ok()?;
    let header = HeaderValue::from_str(authority.as_str()).ok()?;
    Some((authority, header))
}

enum Sender {
    H1(http1::SendRequest<Incoming>),
    H2(http2::SendRequest<Incoming>),
}

impl Sender {
    async fn handshake(tls: TlsStream<TcpStream>) -> hyper::Result<Self> {
        let alpn = negotiated(&tls);
        let io = TokioIo::new(tls);
        match alpn {
            Alpn::H2 => {
                let (sender, connection) = http2::Builder::new(TokioExecutor::new())
                    .initial_stream_window_size(H2_STREAM_WINDOW)
                    .initial_connection_window_size(H2_CONNECTION_WINDOW)
                    .max_send_buf_size(H2_SEND_BUF)
                    .handshake(io)
                    .await?;
                tokio::spawn(async move {
                    if let Err(err) = connection.await {
                        debug!(error = %err, "upstream h2 connection ended with an error");
                    }
                });
                Ok(Sender::H2(sender))
            }
            Alpn::Http11 => {
                let (sender, connection) = http1::Builder::new()
                    .max_buf_size(H1_MAX_BUF)
                    .handshake(io)
                    .await?;
                tokio::spawn(async move {
                    if let Err(err) = connection.await {
                        debug!(error = %err, "upstream http/1.1 connection ended with an error");
                    }
                });
                Ok(Sender::H1(sender))
            }
        }
    }

    fn is_closed(&self) -> bool {
        match self {
            Sender::H1(sender) => sender.is_closed(),
            Sender::H2(sender) => sender.is_closed(),
        }
    }
}

struct Upstream {
    config: Arc<ClientConfig>,
    host: Arc<str>,
    authority: Authority,
    host_header: HeaderValue,
    address: SocketAddr,
    timeout: Duration,
    sender: Mutex<Option<Sender>>,
}

struct AttemptError {
    unsent: Option<Request<Incoming>>,
    error: io::Error,
}

fn failed(error: io::Error) -> AttemptError {
    AttemptError {
        unsent: None,
        error,
    }
}

fn not_sent(error: hyper::Error, request: Request<Incoming>) -> AttemptError {
    AttemptError {
        unsent: Some(request),
        error: io::Error::other(error),
    }
}

fn from_try_send(mut error: hyper::client::conn::TrySendError<Request<Incoming>>) -> AttemptError {
    AttemptError {
        unsent: error.take_message(),
        error: io::Error::other(error.into_error()),
    }
}

impl Upstream {
    async fn send(&self, request: Request<Incoming>) -> io::Result<Response<Incoming>> {
        let (mut parts, body) = request.into_parts();
        strip_hop_by_hop(&mut parts.headers);
        append_via(&mut parts.headers);
        let request = Request::from_parts(parts, body);

        let request = match self.attempt(request).await {
            Ok(response) => return Ok(response),
            Err(AttemptError {
                unsent: Some(request),
                error,
            }) => {
                debug!(host = %self.host, error = %error, "upstream session gone before the request was sent; reconnecting once");
                request
            }
            Err(AttemptError {
                unsent: None,
                error,
            }) => return Err(error),
        };
        self.attempt(request).await.map_err(|attempt| attempt.error)
    }

    async fn attempt(
        &self,
        request: Request<Incoming>,
    ) -> Result<Response<Incoming>, AttemptError> {
        let mut slot = self.sender.lock().await;
        let sender = match slot.as_mut() {
            Some(sender) if !sender.is_closed() => sender,
            _ => {
                let tls =
                    connect_verified_upstream(&self.config, &self.host, self.address, self.timeout)
                        .await
                        .map_err(failed)?;
                let sender = Sender::handshake(tls)
                    .await
                    .map_err(|err| failed(io::Error::other(err)))?;
                slot.insert(sender)
            }
        };
        match sender {
            Sender::H2(sender) => {
                let mut sender = sender.clone();
                drop(slot);
                let request = frame_for(request, Alpn::H2, &self.authority, &self.host_header)
                    .map_err(failed)?;
                if let Err(err) = sender.ready().await {
                    return Err(not_sent(err, request));
                }
                sender
                    .try_send_request(request)
                    .await
                    .map_err(from_try_send)
            }
            Sender::H1(sender) => {
                let request = frame_for(request, Alpn::Http11, &self.authority, &self.host_header)
                    .map_err(failed)?;
                if let Err(err) = sender.ready().await {
                    return Err(not_sent(err, request));
                }
                sender
                    .try_send_request(request)
                    .await
                    .map_err(from_try_send)
            }
        }
    }
}

fn frame_for(
    request: Request<Incoming>,
    alpn: Alpn,
    authority: &Authority,
    host_header: &HeaderValue,
) -> io::Result<Request<Incoming>> {
    let (mut parts, body) = request.into_parts();
    let path = parts
        .uri
        .path_and_query()
        .cloned()
        .unwrap_or_else(|| PathAndQuery::from_static("/"));
    parts.uri = match alpn {
        Alpn::H2 => {
            parts.headers.remove(HOST);
            Uri::builder()
                .scheme(Scheme::HTTPS)
                .authority(authority.clone())
                .path_and_query(path)
                .build()
                .map_err(io::Error::other)?
        }
        Alpn::Http11 => {
            if !parts.headers.contains_key(HOST) {
                parts.headers.insert(HOST, host_header.clone());
            }
            parts.version = Version::HTTP_11;
            Uri::from(path)
        }
    };
    Ok(Request::from_parts(parts, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_comparison_ignores_case_and_a_trailing_dot() {
        assert!(same_host("Example.COM.", "example.com"));
        assert!(!same_host("cdn.example.com", "example.com"));
    }

    #[test]
    fn rejection_status_classifies_exactly_three_alerts() {
        const REJECTED: Option<u16> = Some(CLIENT_CERT_REJECTED);
        let table = [
            (AlertDescription::CloseNotify, None),
            (AlertDescription::UnexpectedMessage, None),
            (AlertDescription::BadRecordMac, None),
            (AlertDescription::DecryptionFailed, None),
            (AlertDescription::RecordOverflow, None),
            (AlertDescription::DecompressionFailure, None),
            (AlertDescription::HandshakeFailure, None),
            (AlertDescription::NoCertificate, None),
            (AlertDescription::BadCertificate, REJECTED),
            (AlertDescription::UnsupportedCertificate, None),
            (AlertDescription::CertificateRevoked, None),
            (AlertDescription::CertificateExpired, None),
            (AlertDescription::CertificateUnknown, REJECTED),
            (AlertDescription::IllegalParameter, None),
            (AlertDescription::UnknownCA, None),
            (AlertDescription::AccessDenied, REJECTED),
            (AlertDescription::DecodeError, None),
            (AlertDescription::DecryptError, None),
            (AlertDescription::ExportRestriction, None),
            (AlertDescription::ProtocolVersion, None),
            (AlertDescription::InsufficientSecurity, None),
            (AlertDescription::InternalError, None),
            (AlertDescription::InappropriateFallback, None),
            (AlertDescription::UserCanceled, None),
            (AlertDescription::NoRenegotiation, None),
            (AlertDescription::MissingExtension, None),
            (AlertDescription::UnsupportedExtension, None),
            (AlertDescription::CertificateUnobtainable, None),
            (AlertDescription::UnrecognisedName, None),
            (AlertDescription::BadCertificateStatusResponse, None),
            (AlertDescription::BadCertificateHashValue, None),
            (AlertDescription::UnknownPSKIdentity, None),
            (AlertDescription::CertificateRequired, None),
            (AlertDescription::NoApplicationProtocol, None),
            (AlertDescription::EncryptedClientHelloRequired, None),
            (AlertDescription::Unknown(0xff), None),
        ];
        for (alert, expected) in table {
            assert_eq!(rejection_status(alert), expected, "{alert:?}");
        }
    }

    #[test]
    fn the_private_codes_are_distinct() {
        assert_ne!(CLIENT_CERT_REJECTED, UPSTREAM_CERT_FAILURE);
        assert_eq!((CLIENT_CERT_REJECTED, UPSTREAM_CERT_FAILURE), (525, 526));
    }
}

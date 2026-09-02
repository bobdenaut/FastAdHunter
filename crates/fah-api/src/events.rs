//! `WS /api/v1/events` — the dashboard's live tail (API.md §Events).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;

use crate::ports::{QueryRecord, StatsSource};
use crate::wire::{QueryItemResponse, StatsResponse};

/// How many events a slow socket may fall behind before it is disconnected.
/// One second of a very busy household at ~250 QPS: enough that a brief
/// scheduling hiccup is absorbed, small enough that a dead socket's buffer
/// stays bounded (hard rule 4).
const CHANNEL_CAPACITY: usize = 256;

/// Cadence of the periodic stats push (API.md: "periodic stats delta, every
/// ~2s").
const STATS_INTERVAL: Duration = Duration::from_secs(2);

/// A send must make progress within this budget or the socket is dropped. A
/// peer that vanishes without closing (a phone leaving Wi-Fi — the normal
/// dashboard client) otherwise parks this task inside `send` once the TCP
/// buffer fills, and the lag-disconnect can never fire because the task is
/// no longer receiving. The 2s stats cadence guarantees traffic to trip
/// this even on an idle network.
const SEND_TIMEOUT: Duration = Duration::from_secs(15);

pub const MAX_CLIENT_MESSAGE_BYTES: usize = 4096;

/// What the server pushes to clients. Serialized as
/// `{ "type": …, "data": … }`.
#[derive(Debug, Clone)]
pub enum Event {
    Query(Box<QueryRecord>),
    ConfigChanged { restart_required: bool },
    ListRefreshed { id: String, status: &'static str },
}

/// The publish side, held by the binary and the route handlers.
#[derive(Clone)]
pub struct EventHub {
    sender: broadcast::Sender<Event>,
    queries: Arc<AtomicUsize>,
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            sender,
            queries: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Publishes a completed event from either pipeline. The binary calls this
    /// from the task that fans events out to stats and metrics; `client_name`
    /// is resolved by the caller (it holds the stats handle).
    ///
    /// One method for both kinds, not two: a dashboard subscribes to "what is
    /// happening", and the `kind` field on the record already says which
    /// pipeline it came from.
    pub fn publish_query(&self, event: fah_model::Event, client_name: Option<String>) {
        self.publish(Event::Query(Box::new(QueryRecord { event, client_name })));
    }

    pub fn publish(&self, event: Event) {
        // `send` fails only when nobody is listening — an idle dashboard is
        // the normal case, not an error.
        let _ = self.sender.send(event);
    }

    pub fn subscribe_socket(&self) -> (broadcast::Receiver<Event>, SocketSubscription) {
        (
            self.sender.subscribe(),
            SocketSubscription::new(Arc::clone(&self.queries)),
        )
    }

    pub fn has_query_subscribers(&self) -> bool {
        self.queries.load(Ordering::Relaxed) > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subscription(u8);

impl Subscription {
    const QUERY: u8 = 1;
    const STATS: u8 = 1 << 1;
    const CONFIG_CHANGED: u8 = 1 << 2;
    const LIST_REFRESHED: u8 = 1 << 3;

    pub const ALL: Self =
        Self(Self::QUERY | Self::STATS | Self::CONFIG_CHANGED | Self::LIST_REFRESHED);

    fn flag(name: &str) -> Option<u8> {
        match name {
            "query" => Some(Self::QUERY),
            "stats" => Some(Self::STATS),
            "config_changed" => Some(Self::CONFIG_CHANGED),
            "list_refreshed" => Some(Self::LIST_REFRESHED),
            _ => None,
        }
    }

    fn parse(frame: &str) -> Option<Self> {
        let message: SubscribeMessage = serde_json::from_str(frame).ok()?;
        let mut flags = 0;
        for name in &message.subscribe {
            flags |= Self::flag(name)?;
        }
        Some(Self(flags))
    }

    fn holds(self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    fn wants_query(self) -> bool {
        self.holds(Self::QUERY)
    }

    fn wants_stats(self) -> bool {
        self.holds(Self::STATS)
    }

    fn wants(self, event: &Event) -> bool {
        self.holds(match event {
            Event::Query(_) => Self::QUERY,
            Event::ConfigChanged { .. } => Self::CONFIG_CHANGED,
            Event::ListRefreshed { .. } => Self::LIST_REFRESHED,
        })
    }
}

#[derive(Deserialize)]
struct SubscribeMessage {
    subscribe: Vec<String>,
}

pub struct SocketSubscription {
    queries: Arc<AtomicUsize>,
    current: Subscription,
}

impl SocketSubscription {
    fn new(queries: Arc<AtomicUsize>) -> Self {
        queries.fetch_add(1, Ordering::Relaxed);
        Self {
            queries,
            current: Subscription::ALL,
        }
    }

    fn current(&self) -> Subscription {
        self.current
    }

    fn set(&mut self, next: Subscription) {
        match (self.current.wants_query(), next.wants_query()) {
            (false, true) => {
                self.queries.fetch_add(1, Ordering::Relaxed);
            }
            (true, false) => {
                self.queries.fetch_sub(1, Ordering::Relaxed);
            }
            _ => {}
        }
        self.current = next;
    }

    fn apply(&mut self, frame: &str) {
        match Subscription::parse(frame) {
            Some(next) => self.set(next),
            None => tracing::debug!(
                frame,
                "ignoring an unusable events subscription frame; the previous set stands"
            ),
        }
    }
}

impl Drop for SocketSubscription {
    fn drop(&mut self) {
        if self.current.wants_query() {
            self.queries.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[derive(Serialize)]
struct Envelope<T> {
    #[serde(rename = "type")]
    kind: &'static str,
    data: T,
}

/// Renders one event as the wire message API.md documents.
pub fn encode(event: Event) -> String {
    match event {
        Event::Query(record) => serde_json::to_string(&Envelope {
            kind: "query",
            data: QueryItemResponse::from(*record),
        }),
        Event::ConfigChanged { restart_required } => serde_json::to_string(&Envelope {
            kind: "config_changed",
            data: json!({ "restart_required": restart_required }),
        }),
        Event::ListRefreshed { id, status } => serde_json::to_string(&Envelope {
            kind: "list_refreshed",
            data: json!({ "id": id, "status": status }),
        }),
    }
    // Every payload here is plain data with no non-string map keys, so
    // serialization cannot fail; the empty object keeps the socket alive in
    // the impossible case rather than tearing it down.
    .unwrap_or_else(|_| "{}".to_string())
}

/// Renders the periodic stats push.
pub fn encode_stats<S: StatsSource + ?Sized>(stats: &S, now: SystemTime) -> String {
    serde_json::to_string(&Envelope {
        kind: "stats",
        data: StatsResponse::from(stats.overview(now)),
    })
    .unwrap_or_else(|_| "{}".to_string())
}

/// Drives one connected socket until it closes or falls behind. Returns when
/// the connection should be dropped.
pub async fn run_socket<S: StatsSource + ?Sized>(
    socket: axum::extract::ws::WebSocket,
    events: broadcast::Receiver<Event>,
    stats: Arc<S>,
    subscription: SocketSubscription,
) {
    drive_socket(socket, events, stats, subscription).await;
}

async fn drive_socket<T, S, E, F>(
    mut socket: T,
    mut events: broadcast::Receiver<Event>,
    stats: Arc<S>,
    mut subscription: SocketSubscription,
) where
    T: futures_util::stream::Stream<Item = Result<axum::extract::ws::Message, E>>
        + futures_util::sink::Sink<axum::extract::ws::Message, Error = F>
        + Unpin,
    S: StatsSource + ?Sized,
{
    use axum::extract::ws::Message;
    use futures_util::SinkExt;

    let mut ticker = tokio::time::interval(STATS_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        let message = tokio::select! {
            received = events.recv() => match received {
                Ok(event) if !subscription.current().wants(&event) => continue,
                Ok(event) => Message::Text(encode(event).into()),
                Err(broadcast::error::RecvError::Lagged(dropped)) => {
                    tracing::debug!(dropped, "disconnecting a slow events subscriber");
                    break;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = ticker.tick() => {
                if subscription.current().wants_stats() {
                    Message::Text(encode_stats(stats.as_ref(), SystemTime::now()).into())
                } else {
                    Message::Ping(Default::default())
                }
            }
            incoming = futures_util::StreamExt::next(&mut socket) => {
                match incoming {
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(Message::Text(frame))) => {
                        subscription.apply(frame.as_str());
                        continue;
                    }
                    Some(Ok(_)) => continue,
                }
            }
        };

        match tokio::time::timeout(SEND_TIMEOUT, socket.send(message)).await {
            Ok(Ok(())) => {}
            // A transport error or a peer that stopped draining: drop it.
            Ok(Err(_)) | Err(_) => break,
        }
    }

    let _ = socket.close().await;
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use fah_model::{DecisiveRule, Query, QueryType, Verdict};
    use serde_json::Value;

    use super::*;

    fn blocked_event() -> fah_model::QueryEvent {
        fah_model::QueryEvent::new(
            Query::new(
                "ads.example.com",
                QueryType::A,
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                SystemTime::UNIX_EPOCH,
            ),
            Verdict::Block(DecisiveRule::new("oisd-basic", "||ads.example.com^")),
            Duration::from_micros(300),
            false,
            false,
            None,
            fah_model::ClientTransport::Udp,
        )
    }

    #[test]
    fn query_event_encodes_with_the_documented_envelope() {
        let json: Value = serde_json::from_str(&encode(Event::Query(Box::new(QueryRecord {
            event: fah_model::Event::dns(blocked_event()),
            client_name: Some("liviu-phone".to_string()),
        }))))
        .unwrap();

        assert_eq!(json["type"], "query");
        assert_eq!(json["data"]["domain"], "ads.example.com");
        assert_eq!(json["data"]["verdict"], "block");
        assert_eq!(json["data"]["client_name"], "liviu-phone");
    }

    fn forwarded_event(endpoint: Option<u8>) -> fah_model::QueryEvent {
        fah_model::QueryEvent::new(
            Query::new(
                "www.example.com",
                QueryType::A,
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                SystemTime::UNIX_EPOCH,
            ),
            Verdict::Pass,
            Duration::from_millis(12),
            endpoint.is_none(),
            endpoint.is_some(),
            None,
            fah_model::ClientTransport::Udp,
        )
        .with_outcome(fah_model::AnswerOutcome::Answered, endpoint)
    }

    #[test]
    fn a_forwarded_query_names_its_answering_endpoint_and_a_cache_hit_does_not() {
        let forwarded: Value = serde_json::from_str(&encode(Event::Query(Box::new(QueryRecord {
            event: fah_model::Event::dns(forwarded_event(Some(1))),
            client_name: None,
        }))))
        .unwrap();
        assert_eq!(forwarded["data"]["endpoint"], 1);
        assert_eq!(forwarded["data"]["cached"], false);

        let cache_hit: Value = serde_json::from_str(&encode(Event::Query(Box::new(QueryRecord {
            event: fah_model::Event::dns(forwarded_event(None)),
            client_name: None,
        }))))
        .unwrap();
        assert_eq!(cache_hit["data"]["cached"], true);
        assert!(
            cache_hit["data"].get("endpoint").is_none(),
            "a cache hit must not carry an endpoint key: {cache_hit}"
        );

        let http: Value = serde_json::from_str(&encode(Event::Query(Box::new(QueryRecord {
            event: fah_model::Event::http(fah_model::RequestEvent::new(
                fah_model::Request {
                    host: "ads.example.com".to_string(),
                    path: "/pixel.gif?id=1".to_string(),
                    method: "GET".to_string(),
                    resource_type: fah_model::ResourceType::Image,
                    client_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                    timestamp: SystemTime::UNIX_EPOCH,
                },
                Verdict::Pass,
                Duration::from_micros(900),
                200,
                0,
            )),
            client_name: None,
        }))))
        .unwrap();
        assert_eq!(http["data"]["kind"], "http");
        assert!(
            http["data"].get("endpoint").is_none(),
            "an HTTP item must not carry an endpoint key: {http}"
        );
    }

    #[test]
    fn control_events_encode_with_their_documented_payloads() {
        let json: Value = serde_json::from_str(&encode(Event::ConfigChanged {
            restart_required: true,
        }))
        .unwrap();
        assert_eq!(json["type"], "config_changed");
        assert_eq!(json["data"]["restart_required"], true);

        let json: Value = serde_json::from_str(&encode(Event::ListRefreshed {
            id: "oisd-basic".to_string(),
            status: "ok",
        }))
        .unwrap();
        assert_eq!(json["type"], "list_refreshed");
        assert_eq!(json["data"]["id"], "oisd-basic");
        assert_eq!(json["data"]["status"], "ok");
    }

    #[tokio::test]
    async fn subscribers_receive_published_events() {
        let hub = EventHub::new();
        let (mut first, _first_guard) = hub.subscribe_socket();
        let (mut second, _second_guard) = hub.subscribe_socket();

        hub.publish_query(fah_model::Event::dns(blocked_event()), None);

        for receiver in [&mut first, &mut second] {
            let event = receiver.recv().await.unwrap();
            assert!(matches!(event, Event::Query(_)));
        }
    }

    #[tokio::test]
    async fn subscriber_presence_is_reported_live() {
        let hub = EventHub::new();
        assert!(!hub.has_query_subscribers(), "idle by default");

        let (_receiver, subscription) = hub.subscribe_socket();
        assert!(
            hub.has_query_subscribers(),
            "a fresh socket defaults to every event, query included"
        );

        drop(subscription);
        assert!(
            !hub.has_query_subscribers(),
            "a disconnected dashboard stops the per-query publish work"
        );
    }

    #[tokio::test]
    async fn narrowing_away_from_query_stops_the_engine_work_without_closing_anything() {
        let hub = EventHub::new();
        let (_receiver, mut subscription) = hub.subscribe_socket();

        subscription.apply(r#"{"subscribe":["stats"]}"#);
        assert!(
            !hub.has_query_subscribers(),
            "a stats-only dashboard must cost the engine what no dashboard costs it"
        );

        subscription.apply(r#"{"subscribe":["query","stats"]}"#);
        assert!(hub.has_query_subscribers(), "widening restores the work");
    }

    #[tokio::test]
    async fn the_count_tracks_sockets_independently() {
        let hub = EventHub::new();
        let (_first_receiver, mut first) = hub.subscribe_socket();
        let (_second_receiver, second) = hub.subscribe_socket();

        first.apply(r#"{"subscribe":[]}"#);
        assert!(
            hub.has_query_subscribers(),
            "the second socket still wants queries"
        );

        drop(second);
        assert!(!hub.has_query_subscribers());

        drop(first);
        assert!(
            !hub.has_query_subscribers(),
            "dropping a socket already narrowed off query must not decrement twice"
        );
    }

    #[test]
    fn a_subscription_message_replaces_the_whole_set() {
        assert_eq!(
            Subscription::parse(r#"{"subscribe":["stats","query"]}"#),
            Some(Subscription(Subscription::STATS | Subscription::QUERY))
        );
        assert_eq!(
            Subscription::parse(r#"{"subscribe":["stats"]}"#),
            Some(Subscription(Subscription::STATS))
        );
        assert_eq!(
            Subscription::parse(r#"{"subscribe":[]}"#),
            Some(Subscription(0)),
            "an empty list is valid and asks for nothing"
        );
    }

    #[test]
    fn an_unusable_frame_leaves_the_previous_set_standing() {
        let queries = Arc::new(AtomicUsize::new(0));
        let mut subscription = SocketSubscription::new(Arc::clone(&queries));
        subscription.apply(r#"{"subscribe":["stats"]}"#);

        for frame in [
            r#"{"subscribe":["stats","nonsense"]}"#,
            r#"{"subscribe":"stats"}"#,
            r#"{"unsubscribe":["query"]}"#,
            "not json at all",
            "",
        ] {
            subscription.apply(frame);
            assert_eq!(
                subscription.current(),
                Subscription(Subscription::STATS),
                "{frame:?} must leave the previous set standing"
            );
        }
        assert_eq!(queries.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn filtering_decides_per_event_kind() {
        let stats_only = Subscription(Subscription::STATS);
        let queries_only = Subscription(Subscription::QUERY);
        let query = Event::Query(Box::new(QueryRecord {
            event: fah_model::Event::dns(blocked_event()),
            client_name: None,
        }));
        let config = Event::ConfigChanged {
            restart_required: false,
        };

        assert!(!stats_only.wants(&query));
        assert!(!stats_only.wants(&config));
        assert!(stats_only.wants_stats());

        assert!(queries_only.wants(&query));
        assert!(!queries_only.wants(&config));
        assert!(!queries_only.wants_stats());

        assert!(Subscription::ALL.wants(&query));
        assert!(Subscription::ALL.wants(&config));
        assert!(Subscription::ALL.wants_stats());
    }

    struct SilentStats;

    impl StatsSource for SilentStats {
        fn overview(&self, _now: SystemTime) -> crate::ports::StatsOverview {
            crate::ports::StatsOverview {
                window: "24h",
                queries_total: 0,
                blocked_total: 0,
                blocked_percent: 0.0,
                cache_hit_percent: 0.0,
                top_blocked_domains: Vec::new(),
                top_queried_domains: Vec::new(),
                top_clients: Vec::new(),
                buckets: Vec::new(),
                policies: Vec::new(),
            }
        }

        fn clients(&self, _now: SystemTime) -> Vec<crate::ports::ClientEntry> {
            Vec::new()
        }

        fn set_client_name(
            &self,
            _ip: IpAddr,
            _name: Option<String>,
        ) -> Option<crate::ports::ClientEntry> {
            None
        }

        fn client_name(&self, _ip: IpAddr) -> Option<String> {
            None
        }

        fn named_clients(&self) -> Vec<(IpAddr, Arc<str>)> {
            Vec::new()
        }

        fn apply_history_config(&self, _enabled: bool, _retention_days: u32) {}

        fn heap(&self) -> fah_model::StatsHeap {
            fah_model::StatsHeap::default()
        }
    }

    type SocketMessage = axum::extract::ws::Message;
    type Incoming = futures_channel::mpsc::UnboundedSender<Result<SocketMessage, axum::Error>>;
    type Outgoing = futures_channel::mpsc::Receiver<SocketMessage>;

    struct TestSocket {
        incoming: futures_channel::mpsc::UnboundedReceiver<Result<SocketMessage, axum::Error>>,
        outgoing: futures_channel::mpsc::Sender<SocketMessage>,
    }

    impl futures_util::stream::Stream for TestSocket {
        type Item = Result<SocketMessage, axum::Error>;

        fn poll_next(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::pin::Pin::new(&mut self.get_mut().incoming).poll_next(cx)
        }
    }

    impl futures_util::sink::Sink<SocketMessage> for TestSocket {
        type Error = futures_channel::mpsc::SendError;

        fn poll_ready(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::pin::Pin::new(&mut self.get_mut().outgoing).poll_ready(cx)
        }

        fn start_send(
            self: std::pin::Pin<&mut Self>,
            item: SocketMessage,
        ) -> Result<(), Self::Error> {
            std::pin::Pin::new(&mut self.get_mut().outgoing).start_send(item)
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::pin::Pin::new(&mut self.get_mut().outgoing).poll_flush(cx)
        }

        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::pin::Pin::new(&mut self.get_mut().outgoing).poll_close(cx)
        }
    }

    fn test_socket(buffer: usize) -> (TestSocket, Incoming, Outgoing) {
        let (incoming_sender, incoming) = futures_channel::mpsc::unbounded();
        let (outgoing, outgoing_receiver) = futures_channel::mpsc::channel(buffer);
        (
            TestSocket { incoming, outgoing },
            incoming_sender,
            outgoing_receiver,
        )
    }

    #[tokio::test]
    async fn a_stats_only_socket_drains_a_burst_that_disconnects_an_unfiltered_one() {
        use futures_util::StreamExt;

        let hub = EventHub::new();
        let stats = Arc::new(SilentStats);

        let (unfiltered_socket, _unfiltered_incoming, mut unfiltered_outgoing) = test_socket(1);
        let (filtered_socket, filtered_incoming, mut filtered_outgoing) = test_socket(1);

        let (unfiltered_events, unfiltered_guard) = hub.subscribe_socket();
        let (filtered_events, mut filtered_guard) = hub.subscribe_socket();
        filtered_guard.apply(r#"{"subscribe":["stats"]}"#);
        assert!(
            hub.has_query_subscribers(),
            "the unfiltered socket wants it"
        );

        let unfiltered = tokio::spawn(drive_socket(
            unfiltered_socket,
            unfiltered_events,
            Arc::clone(&stats),
            unfiltered_guard,
        ));
        let filtered = tokio::spawn(drive_socket(
            filtered_socket,
            filtered_events,
            Arc::clone(&stats),
            filtered_guard,
        ));

        for _ in 0..(CHANNEL_CAPACITY + 64) {
            hub.publish_query(fah_model::Event::dns(blocked_event()), None);
            tokio::task::yield_now().await;
        }

        while unfiltered_outgoing.next().await.is_some() {}
        unfiltered
            .await
            .expect("the unfiltered socket ends by disconnecting, not by panicking");
        assert!(
            !filtered.is_finished(),
            "the stats-only socket drained the same burst without falling behind"
        );
        assert!(
            !hub.has_query_subscribers(),
            "the only socket left is stats-only, so the engine is idle again"
        );

        drop(filtered_incoming);
        filtered
            .await
            .expect("the stats-only socket was still live and closed on the peer's close");

        let mut delivered = Vec::new();
        while let Some(message) = filtered_outgoing.next().await {
            if let SocketMessage::Text(text) = message {
                let event: Value = serde_json::from_str(&text).unwrap();
                delivered.push(event["type"].as_str().unwrap().to_string());
            }
        }
        assert_eq!(
            delivered,
            vec!["stats".to_string()],
            "the stats-only socket emitted its stats push and not one event of the burst"
        );
    }

    #[tokio::test]
    async fn publishing_with_no_subscribers_is_not_an_error() {
        let hub = EventHub::new();
        hub.publish_query(fah_model::Event::dns(blocked_event()), None);
        hub.publish(Event::ConfigChanged {
            restart_required: false,
        });
    }

    #[tokio::test]
    async fn a_subscriber_that_falls_behind_is_told_it_lagged() {
        let hub = EventHub::new();
        let (mut slow, _guard) = hub.subscribe_socket();

        // Overrun the buffer without ever reading: the next read reports the
        // lag, which `run_socket` turns into a disconnect.
        for _ in 0..(CHANNEL_CAPACITY + 10) {
            hub.publish_query(fah_model::Event::dns(blocked_event()), None);
        }

        assert!(matches!(
            slow.recv().await,
            Err(broadcast::error::RecvError::Lagged(_))
        ));
    }
}

//! `WS /api/v1/events` — the dashboard's live tail (API.md §Events).
//!
//! One `tokio::broadcast` channel fans every event out to all connected
//! sockets. Broadcast is deliberate: it drops for a receiver that falls
//! behind instead of back-pressuring the sender, which is exactly API.md's
//! "slow consumers are disconnected rather than back-pressuring the engine" —
//! a lagging socket sees `RecvError::Lagged` and we close it.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use fah_model::QueryEvent;
use serde::Serialize;
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
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self { sender }
    }

    /// Publishes a completed query. The binary calls this from the task that
    /// fans `QueryEvent`s out to stats and metrics. `client_name` is resolved
    /// by the caller (it holds the stats handle).
    pub fn publish_query(&self, event: QueryEvent, client_name: Option<String>) {
        self.publish(Event::Query(Box::new(QueryRecord { event, client_name })));
    }

    pub fn publish(&self, event: Event) {
        // `send` fails only when nobody is listening — an idle dashboard is
        // the normal case, not an error.
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    /// Whether any events socket is currently connected. The binary's
    /// fan-out checks this before doing per-query publish work (client-name
    /// lookup, boxing the record) that the hub would otherwise throw away —
    /// and no-dashboard-connected is the appliance's idle state ~24h/day.
    pub fn has_subscribers(&self) -> bool {
        self.sender.receiver_count() > 0
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
    mut socket: axum::extract::ws::WebSocket,
    mut events: broadcast::Receiver<Event>,
    stats: Arc<S>,
) {
    use axum::extract::ws::Message;
    use futures_util::SinkExt;

    let mut ticker = tokio::time::interval(STATS_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        let text = tokio::select! {
            received = events.recv() => match received {
                Ok(event) => encode(event),
                // The engine outran this socket: disconnect rather than let
                // the backlog grow (API.md §Events).
                Err(broadcast::error::RecvError::Lagged(dropped)) => {
                    tracing::debug!(dropped, "disconnecting a slow events subscriber");
                    break;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = ticker.tick() => encode_stats(stats.as_ref(), SystemTime::now()),
            incoming = futures_util::StreamExt::next(&mut socket) => {
                match incoming {
                    // Clients are not expected to send anything; a close or a
                    // transport error ends the connection.
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => continue,
                }
            }
        };

        match tokio::time::timeout(SEND_TIMEOUT, socket.send(Message::Text(text.into()))).await {
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

    fn blocked_event() -> QueryEvent {
        QueryEvent::new(
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
            false,
        )
    }

    #[test]
    fn query_event_encodes_with_the_documented_envelope() {
        let json: Value = serde_json::from_str(&encode(Event::Query(Box::new(QueryRecord {
            event: blocked_event(),
            client_name: Some("liviu-phone".to_string()),
        }))))
        .unwrap();

        assert_eq!(json["type"], "query");
        assert_eq!(json["data"]["domain"], "ads.example.com");
        assert_eq!(json["data"]["verdict"], "block");
        assert_eq!(json["data"]["client_name"], "liviu-phone");
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
        let mut first = hub.subscribe();
        let mut second = hub.subscribe();

        hub.publish_query(blocked_event(), None);

        for receiver in [&mut first, &mut second] {
            let event = receiver.recv().await.unwrap();
            assert!(matches!(event, Event::Query(_)));
        }
    }

    #[tokio::test]
    async fn subscriber_presence_is_reported_live() {
        let hub = EventHub::new();
        assert!(!hub.has_subscribers(), "idle by default");

        let receiver = hub.subscribe();
        assert!(hub.has_subscribers());

        drop(receiver);
        assert!(
            !hub.has_subscribers(),
            "a disconnected dashboard stops the per-query publish work"
        );
    }

    #[tokio::test]
    async fn publishing_with_no_subscribers_is_not_an_error() {
        let hub = EventHub::new();
        hub.publish_query(blocked_event(), None);
        hub.publish(Event::ConfigChanged {
            restart_required: false,
        });
    }

    #[tokio::test]
    async fn a_subscriber_that_falls_behind_is_told_it_lagged() {
        let hub = EventHub::new();
        let mut slow = hub.subscribe();

        // Overrun the buffer without ever reading: the next read reports the
        // lag, which `run_socket` turns into a disconnect.
        for _ in 0..(CHANNEL_CAPACITY + 10) {
            hub.publish_query(blocked_event(), None);
        }

        assert!(matches!(
            slow.recv().await,
            Err(broadcast::error::RecvError::Lagged(_))
        ));
    }
}

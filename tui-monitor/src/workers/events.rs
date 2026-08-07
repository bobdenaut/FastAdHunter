//! The live feed: reconnects the events socket and routes each typed frame to
//! the part of the state it belongs to.

use std::time::Duration;

use futures_util::StreamExt;

use crate::client::EventsClient;
use crate::models::events::{Decoded, ServerEvent};
use crate::state::{LinkStatus, SharedState};

pub async fn run(
    client: EventsClient,
    reconnect_after: Duration,
    idle_after: Duration,
    state: SharedState,
) {
    loop {
        match client.connect().await {
            Ok(stream) => {
                state.update(|app| app.events = LinkStatus::Online);
                let ended = consume(stream, idle_after, &state).await;
                state.update(|app| app.events = LinkStatus::Down(ended.to_string()));
            }
            Err(error) => state.update(|app| app.events = LinkStatus::Down(error)),
        }

        tokio::time::sleep(reconnect_after).await;
    }
}

/// Why a connection stopped being useful.
enum Ended {
    /// The stream finished — a clean end is still a disconnect.
    Closed,
    /// Nothing arrived for the idle budget. The server pushes `stats` every
    /// ~2 s, so silence means the socket is dead even though the OS still
    /// believes it is open — a half-open TCP after a reboot or a NAT eviction.
    Silent(Duration),
}

impl std::fmt::Display for Ended {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "disconnected"),
            Self::Silent(after) => write!(f, "silent {}s", after.as_secs()),
        }
    }
}

async fn consume(
    stream: impl futures_util::Stream<Item = Decoded>,
    idle_after: Duration,
    state: &SharedState,
) -> Ended {
    let mut stream = std::pin::pin!(stream);

    loop {
        match tokio::time::timeout(idle_after, stream.next()).await {
            Ok(Some(frame)) => apply(frame, state),
            Ok(None) => return Ended::Closed,
            Err(_) => return Ended::Silent(idle_after),
        }
    }
}

fn apply(frame: Decoded, state: &SharedState) {
    match frame {
        Decoded::Event(ServerEvent::Query(item)) => state.update(|app| app.push_query(*item)),
        Decoded::Event(ServerEvent::Stats(stats)) => state.update(|app| app.live = *stats),
        Decoded::Undecodable => state.update(|app| {
            app.undecodable_frames = app.undecodable_frames.saturating_add(1);
        }),
        Decoded::Unrendered => {}
    }
}

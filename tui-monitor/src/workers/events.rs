//! The live feed: reconnects the events socket and routes each typed frame to
//! the part of the state it belongs to.

use std::time::Duration;

use futures_util::StreamExt;

use crate::client::EventsClient;
use crate::models::events::ServerEvent;
use crate::state::{LinkStatus, SharedState};

pub async fn run(client: EventsClient, reconnect_after: Duration, state: SharedState) {
    loop {
        match client.connect().await {
            Ok(stream) => {
                state.update(|app| app.events = LinkStatus::Online);
                consume(stream, &state).await;
                // A clean end of stream is still a disconnect.
                state.update(|app| {
                    app.events = LinkStatus::Down("disconnected".to_string());
                });
            }
            Err(error) => state.update(|app| app.events = LinkStatus::Down(error)),
        }

        tokio::time::sleep(reconnect_after).await;
    }
}

async fn consume(stream: impl futures_util::Stream<Item = ServerEvent>, state: &SharedState) {
    let mut stream = std::pin::pin!(stream);

    while let Some(event) = stream.next().await {
        match event {
            ServerEvent::Query(item) => state.update(|app| app.push_query(*item)),
            ServerEvent::Stats(stats) => state.update(|app| app.live = *stats),
        }
    }
}

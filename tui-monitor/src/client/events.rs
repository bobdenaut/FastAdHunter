//! The `WS /api/v1/events` socket, as a stream of typed frames.
//!
//! The client's whole job is: connect, and hand back something that yields
//! [`ServerEvent`]s. Reconnection is a *policy*, so it lives in the worker.

use std::sync::Arc;

use futures_util::{Stream, StreamExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::Connector;

use crate::config::Config;
use crate::models::events::Decoded;

use super::api::paths;

pub struct EventsClient {
    url: String,
    bearer: String,
    connector: Option<Arc<rustls::ClientConfig>>,
}

impl EventsClient {
    pub fn new(config: &Config, tls: &super::TlsPolicy) -> Result<Self, std::io::Error> {
        Ok(Self {
            url: config.websocket_url(paths::EVENTS),
            bearer: format!("Bearer {}", config.token),
            connector: tls.websocket_connector(),
        })
    }

    /// Opens one socket. The caller owns what happens when it ends.
    ///
    /// Every text frame is yielded as a [`Decoded`], undecodable ones included:
    /// a bad frame must not drop a live feed, but dropping it *silently* is how
    /// a server-side rename empties the feed with the socket still up.
    pub async fn connect(&self) -> Result<impl Stream<Item = Decoded>, String> {
        let mut request = self
            .url
            .as_str()
            .into_client_request()
            .map_err(|err| err.to_string())?;

        // In a header rather than a `?token=` query parameter: a URL lands in
        // logs and shell history.
        request.headers_mut().insert(
            "Authorization",
            self.bearer
                .parse()
                .map_err(|_| "invalid token".to_string())?,
        );

        let connector = self.connector.clone().map(Connector::Rustls);
        let (socket, _) =
            tokio_tungstenite::connect_async_tls_with_config(request, None, false, connector)
                .await
                .map_err(|err| err.to_string())?;

        Ok(socket.filter_map(|message| async move {
            match message {
                Ok(Message::Text(text)) => Some(crate::models::events::decode(&text)),
                _ => None,
            }
        }))
    }
}

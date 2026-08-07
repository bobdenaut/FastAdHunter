//! The single poller of `GET /api/v1/telemetry` — ruleset, counters, latency,
//! upstreams, cache and memory in one request.
//!
//! It does not touch the RSS graph: that series belongs to `/history/perf`, and
//! a locally sampled point would put this process's clock and cadence on the
//! appliance's axis.

use crate::client::ApiClient;
use crate::config::PollConfig;
use crate::state::{LinkStatus, SharedState};

pub async fn run(client: ApiClient, poll: PollConfig, state: SharedState) {
    let mut ticker = tokio::time::interval(poll.telemetry());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        match client.telemetry().await {
            Ok(telemetry) => state.update(|app| {
                app.api = LinkStatus::Online;
                app.telemetry = Some(telemetry);
            }),
            // The error is kept so the header can say why the figures stopped
            // moving; the last good reading stays on screen.
            Err(error) => state.update(|app| app.api = LinkStatus::Down(error.to_string())),
        }
    }
}

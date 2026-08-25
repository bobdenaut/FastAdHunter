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

    let mut last_uptime = 0;

    loop {
        ticker.tick().await;

        match client.telemetry().await {
            Ok(telemetry) => {
                let uptime = telemetry.process.uptime_seconds;
                let restarted = uptime < last_uptime;
                last_uptime = uptime;

                let stale = needs_config(restarted, state.read().strategy.as_deref());

                state.update(|app| {
                    app.api = LinkStatus::Online;
                    app.telemetry = Some(telemetry);
                    if stale {
                        app.strategy = None;
                    }
                });

                if stale {
                    if let Ok(config) = client.config().await {
                        state.update(|app| app.strategy = Some(config.dns.upstreams.strategy));
                    }
                }
            }
            // The error is kept so the header can say why the figures stopped
            // moving; the last good reading stays on screen.
            Err(error) => state.update(|app| app.api = LinkStatus::Down(error.to_string())),
        }
    }
}

fn needs_config(restarted: bool, held: Option<&str>) -> bool {
    restarted || held.is_none()
}

#[cfg(test)]
mod tests {
    use super::needs_config;

    #[test]
    fn the_first_poll_reads_the_config() {
        assert!(needs_config(false, None));
    }

    #[test]
    fn a_held_strategy_is_not_reread_every_poll() {
        assert!(!needs_config(false, Some("adaptive")));
    }

    #[test]
    fn a_restart_rereads_even_when_a_strategy_is_held() {
        assert!(needs_config(true, Some("fallback")));
    }

    #[test]
    fn a_failed_read_is_retried_because_the_held_value_was_cleared() {
        assert!(needs_config(true, Some("fallback")));
        assert!(needs_config(false, None));
    }
}

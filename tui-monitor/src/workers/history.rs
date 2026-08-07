//! The persisted series: the Today and Last-7-Days panels, and the header's RSS
//! graph.
//!
//! Two tasks, because they answer different questions on different cadences. A
//! summary bucket is a completed *hour*, so re-reading it every few minutes
//! would return the same rows; the perf series gains a point every 60 s.

use crate::client::ApiClient;
use crate::config::PollConfig;
use crate::state::{SharedState, Window};

/// Today and the last seven days.
pub async fn run_summary(client: ApiClient, poll: PollConfig, state: SharedState) {
    let mut ticker = tokio::time::interval(poll.history());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        if let Ok(summary) = client.history_today().await {
            let window = Window::from_summary(&summary);
            state.update(|app| app.today = Some(window));
        }

        if let Ok(summary) = client.history_since(&seven_days_ago()).await {
            let window = Window::from_summary(&summary);
            state.update(|app| app.week = Some(window));
        }
    }
}

/// The RSS graph, re-read whole from `/history/perf` on every tick, so the
/// series always carries the appliance's own points at its own cadence.
pub async fn run_perf(client: ApiClient, poll: PollConfig, state: SharedState) {
    let mut ticker = tokio::time::interval(poll.rss_history());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        if let Ok(perf) = client.history_perf().await {
            // Rows whose `rss_bytes` was not stored are skipped rather than
            // charted as zero, which would draw a cliff that never happened.
            let samples: Vec<u64> = perf.items.iter().filter_map(|it| it.rss_bytes).collect();
            state.update(|app| app.set_rss_history(&samples));
        }
    }
}

/// Midnight UTC, seven days back — the `from` bound of the weekly panel.
fn seven_days_ago() -> String {
    chrono::Utc::now()
        .date_naive()
        .checked_sub_days(chrono::Days::new(7))
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|at| at.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        // Unreachable for any clock the process can actually observe; falling
        // back to the epoch asks for everything rather than failing the panel.
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_weekly_bound_is_rfc_3339_at_midnight_utc() {
        let from = seven_days_ago();
        assert!(from.ends_with("T00:00:00Z"), "{from}");
        assert!(
            chrono::DateTime::parse_from_rfc3339(&from).is_ok(),
            "{from}"
        );
    }
}

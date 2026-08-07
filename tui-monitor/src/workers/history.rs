//! The persisted series: the Today and Last-7-Days panels, and the header's RSS
//! graph.
//!
//! Two tasks, because they answer different questions on different cadences. A
//! summary bucket is a completed *hour*, so re-reading it every few minutes
//! would return the same rows; the perf series gains a point every 60 s.

use crate::client::ApiClient;
use crate::config::PollConfig;
use crate::state::{LinkStatus, SharedState, Window};

/// Whole UTC days the weekly panel covers, today included.
const WEEK_DAYS: u64 = 7;

/// The rolling last-24 h, and the last seven days.
pub async fn run_summary(client: ApiClient, poll: PollConfig, state: SharedState) {
    let mut ticker = tokio::time::interval(poll.history());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        // Recomputed each tick: the bound moves when the UTC day rolls over.
        let from = window_start(WEEK_DAYS);
        // Concurrent: two independent reads of one endpoint, and the tick
        // budget is for both together.
        let (recent, week) = tokio::join!(client.history_last_24h(), client.history_since(&from));
        // Aggregated before the lock is taken, never under it.
        let recent = recent.map(|summary| Window::from_summary(&summary));
        let week = week.map(|summary| Window::from_summary(&summary));

        state.update(|app| {
            app.history = match (&recent, &week) {
                (Err(error), _) | (_, Err(error)) => LinkStatus::Down(error.to_string()),
                _ => LinkStatus::Online,
            };
            // Each half is applied only when it succeeded: one failing read
            // must not blank the panel the other just refreshed.
            if let Ok(window) = recent {
                app.last_24h = Some(window);
            }
            if let Ok(window) = week {
                app.week = Some(window);
            }
        });
    }
}

/// The RSS graph, re-read whole from `/history/perf` on every tick, so the
/// series always carries the appliance's own points at its own cadence.
pub async fn run_perf(client: ApiClient, poll: PollConfig, state: SharedState) {
    let mut ticker = tokio::time::interval(poll.rss_history());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        match client.history_perf().await {
            Ok(perf) => {
                // Rows whose `rss_bytes` was not stored are skipped rather than
                // charted as zero, which would draw a cliff that never happened.
                let samples: Vec<u64> = perf.items.iter().filter_map(|it| it.rss_bytes).collect();
                state.update(|app| {
                    app.perf = LinkStatus::Online;
                    app.set_rss_history(&samples, perf.stride);
                });
            }
            // Kept, so the graph can say why it stopped advancing instead of
            // showing an hours-old series as though it were current.
            Err(error) => state.update(|app| app.perf = LinkStatus::Down(error.to_string())),
        }
    }
}

/// The `from` bound of a window covering `days` whole UTC days, today included.
fn window_start(days: u64) -> String {
    chrono::Utc::now()
        .date_naive()
        // `days - 1` back, because today is one of them. Going the full `days`
        // back returns one bucket too many — an eight-day "last 7 days".
        .checked_sub_days(chrono::Days::new(days.saturating_sub(1)))
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
        let from = window_start(WEEK_DAYS);
        assert!(from.ends_with("T00:00:00Z"), "{from}");
        assert!(
            chrono::DateTime::parse_from_rfc3339(&from).is_ok(),
            "{from}"
        );
    }

    /// `[from, now)` at day resolution yields one bucket per UTC day it spans,
    /// today included — so a 7-day panel starts **6** days back. Asking for 7
    /// back is what made the panel serve eight buckets under a seven-day title.
    #[test]
    fn a_seven_day_window_spans_seven_buckets_not_eight() {
        let from = chrono::NaiveDate::parse_from_str(
            window_start(WEEK_DAYS).trim_end_matches("T00:00:00Z"),
            "%Y-%m-%d",
        )
        .unwrap();
        let today = chrono::Utc::now().date_naive();

        let buckets = (today - from).num_days() + 1;
        assert_eq!(buckets, WEEK_DAYS as i64, "from {from} through {today}");
    }
}

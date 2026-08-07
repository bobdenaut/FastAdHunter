//! The one thing workers write and the UI reads. Two rules hold the layering:
//!
//! 1. **Only workers write.** A drawing function takes `&AppState`, so scroll
//!    offsets, the open popup and the table cursor live in
//!    [`crate::app::UiState`] instead — a keypress takes no lock here.
//! 2. **Nothing is stored twice.** Responses are kept parsed and read from;
//!    no figure is copied out into a flat field or pre-formatted.

use std::collections::VecDeque;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use crate::config::Limits;
use crate::models::events::{QueryItem, StatsPush};
use crate::models::history::HistorySummary;
use crate::models::telemetry::Telemetry;
use crate::util::format::percent;

/// Whether a data source is currently answering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LinkStatus {
    #[default]
    Connecting,
    Online,
    /// Carries what went wrong, so a wrong token reads differently from an
    /// unplugged cable.
    Down(String),
}

impl LinkStatus {
    pub fn is_online(&self) -> bool {
        matches!(self, Self::Online)
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Connecting => "CONNECTING",
            Self::Online => "ONLINE",
            Self::Down(reason) => reason,
        }
    }
}

#[derive(Debug, Default)]
pub struct AppState {
    /// Ring bounds from `[ui]`. Held here because they bound *this* structure —
    /// nothing else in the program may grow with uptime.
    limits: Limits,

    /// The `/api/v1/telemetry` poller.
    pub api: LinkStatus,
    /// The events websocket.
    pub events: LinkStatus,

    /// The last complete engine read. `None` until the first poll returns,
    /// which is a different thing from a genuine zero.
    pub telemetry: Option<Telemetry>,
    /// The ~2 s push riding the events socket.
    pub live: StatsPush,
    /// Newest first.
    pub queries: VecDeque<QueryItem>,

    pub today: Option<Window>,
    pub week: Option<Window>,

    /// Resident set in MiB, oldest first, straight from `/history/perf`.
    pub rss_history: VecDeque<f64>,

    pub router: RouterStatus,
}

impl AppState {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            ..Default::default()
        }
    }

    pub fn push_query(&mut self, item: QueryItem) {
        self.queries.push_front(item);
        while self.queries.len() > self.limits.feed_rows {
            self.queries.pop_back();
        }
    }

    /// Sets the graph to the appliance's persisted series, keeping the newest
    /// `rss_points`. Whole-series replacement, never an append: every point
    /// drawn is one `/history/perf` recorded, on one cadence and one clock.
    pub fn set_rss_history(&mut self, samples: &[u64]) {
        let newest = samples.len().saturating_sub(self.limits.rss_points);
        self.rss_history = samples[newest..]
            .iter()
            .map(|bytes| crate::util::format::mib(*bytes))
            .collect();
    }

    /// The graph's series. Empty until the first `/history/perf` read returns —
    /// the header draws a blank chart rather than inventing a point.
    pub fn rss_series(&self) -> Vec<f64> {
        self.rss_history.iter().copied().collect()
    }
}

/// An aggregated history window — one type for both the Today and the
/// Last-7-Days panel, built by summing whatever buckets the API returned.
#[derive(Debug, Clone, Default)]
pub struct Window {
    pub queries: u64,
    pub blocked: u64,
    pub cache_hits: u64,
    /// Record type → count, over the whole window.
    pub per_type: std::collections::BTreeMap<String, u64>,
    /// Queries per bucket in order, for the panel's sparkline.
    pub series: Vec<u64>,
    /// Bucket width the API served — `hour` or `day`.
    pub resolution: String,
    /// `1` when every bucket is present, `n` when only every `n`-th survived
    /// the point budget. Shown, because a silently sparse chart is a lying one.
    pub stride: u64,
}

impl Window {
    pub fn from_summary(summary: &HistorySummary) -> Self {
        let mut window = Self {
            resolution: summary.resolution.clone(),
            stride: summary.stride,
            ..Default::default()
        };
        for point in &summary.items {
            window.queries += point.queries;
            window.blocked += point.blocked;
            window.cache_hits += point.cache_hits;
            for (kind, count) in &point.per_type {
                *window.per_type.entry(kind.clone()).or_default() += count;
            }
            window.series.push(point.queries);
        }
        window
    }

    /// How the buckets were served, and whether any were dropped.
    pub fn coverage(&self) -> String {
        match self.stride {
            0 | 1 => format!("{} buckets, {}ly", self.series.len(), self.resolution),
            stride => format!(
                "{} buckets, {}ly, 1 in {stride}",
                self.series.len(),
                self.resolution
            ),
        }
    }

    pub fn percent_of_queries(&self, part: u64) -> f64 {
        percent(part, self.queries)
    }

    /// Record types most-used first. The label set is whatever the window
    /// contains, so the panel hard-codes none of them.
    pub fn types_by_count(&self) -> Vec<(&str, u64)> {
        let mut types: Vec<(&str, u64)> = self
            .per_type
            .iter()
            .map(|(kind, count)| (kind.as_str(), *count))
            .collect();
        types.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        types
    }
}

#[derive(Debug, Clone, Default)]
pub struct RouterStatus {
    pub free_memory: Option<u64>,
    pub total_memory: Option<u64>,
    pub cpu_load: Option<u64>,
    pub cpu_frequency: Option<u64>,
    pub container_memory: Option<u64>,
    pub container_status: Option<String>,
    /// RouterOS's own spelling, e.g. `3d04:12:55` — passed through unparsed.
    pub uptime: Option<String>,
}

/// Shared handle to [`AppState`]. Cloning it is cloning an `Arc`.
///
/// A poisoned lock is recovered rather than propagated: one worker panicking
/// must not take the display down with it.
#[derive(Clone)]
pub struct SharedState(Arc<RwLock<AppState>>);

impl SharedState {
    pub fn new(limits: Limits) -> Self {
        Self(Arc::new(RwLock::new(AppState::new(limits))))
    }

    pub fn read(&self) -> RwLockReadGuard<'_, AppState> {
        self.0
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Held for exactly the closure. Workers do their parsing and aggregating
    /// first, then take the lock to store the finished value.
    pub fn update(&self, edit: impl FnOnce(&mut AppState)) {
        let mut guard = self
            .0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        edit(&mut guard);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::models::history::HistoryPoint;

    /// `(queries, blocked, per-type counts)` for one bucket.
    type Bucket<'a> = (u64, u64, &'a [(&'a str, u64)]);

    fn summary(stride: u64, points: &[Bucket]) -> HistorySummary {
        HistorySummary {
            resolution: "hour".to_string(),
            stride,
            items: points
                .iter()
                .map(|(queries, blocked, types)| HistoryPoint {
                    queries: *queries,
                    blocked: *blocked,
                    cache_hits: queries / 4,
                    per_type: types
                        .iter()
                        .map(|(k, v)| ((*k).to_string(), *v))
                        .collect::<BTreeMap<_, _>>(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_window_sums_its_buckets_and_merges_the_type_maps() {
        let window = Window::from_summary(&summary(
            1,
            &[
                (100, 60, &[("A", 70), ("AAAA", 30)]),
                (300, 90, &[("A", 200), ("HTTPS", 100)]),
            ],
        ));

        assert_eq!(window.queries, 400);
        assert_eq!(window.blocked, 150);
        assert_eq!(window.cache_hits, 100);
        assert_eq!(window.percent_of_queries(window.blocked), 37.5);
        assert_eq!(
            window.types_by_count(),
            vec![("A", 270), ("HTTPS", 100), ("AAAA", 30)]
        );
        assert_eq!(window.series, vec![100, 300]);
    }

    #[test]
    fn an_empty_window_reports_zero_rather_than_dividing_by_nothing() {
        let window = Window::from_summary(&summary(1, &[]));
        assert_eq!(window.percent_of_queries(0), 0.0);
        assert!(window.types_by_count().is_empty());
    }

    /// A decimated series must say so: the same chart drawn from every third
    /// bucket looks identical to one drawn from all of them.
    #[test]
    fn a_decimated_window_reports_the_stride_it_was_served_at() {
        assert_eq!(
            Window::from_summary(&summary(1, &[(1, 0, &[])])).coverage(),
            "1 buckets, hourly"
        );
        assert_eq!(
            Window::from_summary(&summary(3, &[(1, 0, &[]), (2, 0, &[])])).coverage(),
            "2 buckets, hourly, 1 in 3"
        );
    }

    #[test]
    fn the_query_ring_honours_the_configured_bound_and_keeps_the_newest() {
        let limits = Limits {
            feed_rows: 4,
            rss_points: 8,
        };
        let mut state = AppState::new(limits);
        for index in 0..20 {
            state.push_query(query(index));
        }

        assert_eq!(state.queries.len(), 4);
        assert_eq!(state.queries[0].domain, "d19", "newest first");
        assert_eq!(state.queries[3].domain, "d16");
    }

    /// A longer series than the graph holds must keep its **newest** points —
    /// dropping from the wrong end would show a day-old window forever.
    #[test]
    fn the_rss_graph_keeps_the_newest_points_within_its_bound() {
        let mut state = AppState::new(Limits {
            feed_rows: 10,
            rss_points: 3,
        });
        let samples: Vec<u64> = (1..=10).map(|n| n * 1_048_576).collect();
        state.set_rss_history(&samples);

        assert_eq!(state.rss_series(), vec![8.0, 9.0, 10.0]);
    }

    /// The series is replaced, never appended to: a second read that returns
    /// fewer rows must shrink the graph rather than leave stale points behind.
    #[test]
    fn a_later_read_replaces_the_series_rather_than_extending_it() {
        let mut state = AppState::default();
        state.set_rss_history(&[1_048_576, 2_097_152, 3_145_728]);
        state.set_rss_history(&[4_194_304]);

        assert_eq!(state.rss_series(), vec![4.0]);
    }

    /// Before the first poll the graph still has to draw something, and a hard
    /// zero would read as "RSS collapsed" rather than "no data yet".
    #[test]
    fn the_rss_series_is_empty_before_anything_has_been_read() {
        assert!(AppState::default().rss_series().is_empty());
    }

    fn query(index: usize) -> QueryItem {
        QueryItem {
            kind: "dns".to_string(),
            ts: String::new(),
            client: std::net::IpAddr::from([10, 0, 0, 1]),
            client_name: None,
            domain: format!("d{index}"),
            qtype: Some("A".to_string()),
            verdict: crate::models::events::Verdict::Pass,
            rule: None,
            list: None,
            duration_ms: 0.1,
            cached: false,
            method: None,
            path: None,
            status: None,
            bytes: None,
        }
    }
}

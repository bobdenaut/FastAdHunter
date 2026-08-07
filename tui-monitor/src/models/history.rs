//! `GET /api/v1/history/summary` and `/history/perf` (API.md §History).
//!
//! The summary backs the Today and Last-7-Days panels; the perf series is the
//! header's RSS graph.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct HistorySummary {
    /// `hour` | `day`.
    pub resolution: String,
    /// `1` when every stored point is present, `n` when only every `n`-th
    /// survived the point budget.
    pub stride: u64,
    pub items: Vec<HistoryPoint>,
}

/// One bucket — a completed hour, or a UTC day at `resolution=day`.
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryPoint {
    pub queries: u64,
    pub blocked: u64,
    pub cache_hits: u64,
    /// Canonical DNS type → count. Zero buckets are omitted, and `OTHER` lumps
    /// everything outside the fixed label set.
    pub per_type: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HistoryPerf {
    /// `1` when every stored sample is present, `n` when only every `n`-th
    /// survived the point budget. Carried for the same reason
    /// [`HistorySummary::stride`] is: a sparse graph that does not say so is a
    /// graph claiming a time span it never covered.
    pub stride: u64,
    pub items: Vec<PerfPoint>,
}

/// One perf sample. Requested as `?fields=rss_bytes`, so every other key is
/// absent rather than null.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PerfPoint {
    #[serde(default)]
    pub rss_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::fixtures;

    #[test]
    fn a_summary_page_parses_with_its_per_type_map() {
        let summary: HistorySummary = serde_json::from_str(fixtures::HISTORY_SUMMARY).unwrap();

        assert_eq!(summary.resolution, "hour");
        assert_eq!(summary.stride, 1);
        assert_eq!(summary.items.len(), 3);
        assert_eq!(summary.items[0].queries, 5312);
        assert_eq!(summary.items[0].per_type["HTTPS"], 432);
    }

    /// `?fields=rss_bytes` drops every other key from each row.
    #[test]
    fn a_field_filtered_perf_page_parses_without_the_dropped_keys() {
        let perf: HistoryPerf = serde_json::from_str(fixtures::HISTORY_PERF).unwrap();

        assert_eq!(perf.stride, 1);
        assert_eq!(perf.items.len(), 4);
        assert_eq!(perf.items[0].rss_bytes, Some(53_907_456));
    }

    /// A row written before the sampler had published carries no `rss_bytes`
    /// at all, which must read as absent rather than fail the page.
    #[test]
    fn a_row_without_the_requested_field_is_absent_not_an_error() {
        let perf: HistoryPerf =
            serde_json::from_str(r#"{"stride":1,"items":[{"ts":"a"},{"ts":"b","rss_bytes":1}]}"#)
                .unwrap();

        assert_eq!(perf.items[0].rss_bytes, None);
        assert_eq!(perf.items[1].rss_bytes, Some(1));
    }
}

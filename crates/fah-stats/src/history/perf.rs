//! Append-only per-interval perf/system/cache sample writer on
//! `/data/history/perf` (ADR-0002: flat JSONL, no embedded DB). One
//! [`PerfSample`] line per sampling interval, in a day-file
//! `perf-YYYY-MM-DD.jsonl`, pruned by age like the rollups.
//!
//! Unlike the rollup writer there is no completion cursor: each sample is a
//! fresh, independent reading (not re-derived from a ring), so a restart just
//! continues appending — no idempotency to enforce, and re-persisting can't
//! grow the file for a fixed instant (hard rule 4). Memory is fixed: the
//! writer holds a directory path, a retention bound, and one prune cursor.
//!
//! Sampling and the sample's *contents* live in the binary (it alone can read
//! `fah-metrics`, the cache port and RSS — siblings never import each other,
//! ARCHITECTURE.md §Dependency Layering); this writer only persists the plain
//! data it's handed, off the per-query path (hard rule 3).

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fah_model::PerfSample;
use tokio::fs;

use crate::history::append_line;
use crate::history::date::{date_string, parse_day};

const SECS_PER_DAY: u64 = 86_400;

/// Fallback prune cadence (mirrors the rollup writer): often enough that
/// `retention_days` doesn't lag by more than an hour, rare enough that the
/// directory scan disappears from the steady state.
const PRUNE_INTERVAL: Duration = Duration::from_secs(3600);

pub(crate) struct PerfWriter {
    dir: PathBuf,
    /// Shared with the rollup writer and [`Stats`](crate::Stats) so one
    /// `POST /api/v1/config` setter updates both writers' retention live (hard
    /// rule 3: atomic swap, no reconstruction).
    retention_days: Arc<AtomicU32>,
    last_prune: Option<Instant>,
}

impl PerfWriter {
    pub(crate) fn new(dir: PathBuf, retention_days: Arc<AtomicU32>) -> Self {
        Self {
            dir,
            retention_days,
            last_prune: None,
        }
    }

    pub(crate) async fn boot(&mut self) -> io::Result<()> {
        fs::create_dir_all(&self.dir).await
    }

    /// Appends one sample as a JSONL line into the day-file named for the
    /// sample's own timestamp (so a sample captured just before midnight lands
    /// in the correct day even if the flush runs after).
    pub(crate) async fn append(&mut self, sample: &PerfSample) -> io::Result<()> {
        let line = serde_json::to_string(sample).map_err(io::Error::other)?;
        append_line(&self.perf_path(sample.ts / SECS_PER_DAY), &line).await
    }

    /// Prunes at most once per [`PRUNE_INTERVAL`] (and always on the first call
    /// after boot), keeping the steady-state tick scan-free.
    pub(crate) async fn maybe_prune(&mut self, now: SystemTime) -> io::Result<()> {
        let due = self
            .last_prune
            .is_none_or(|at| at.elapsed() >= PRUNE_INTERVAL);
        if !due {
            return Ok(());
        }
        self.last_prune = Some(Instant::now());
        self.prune(now).await
    }

    /// Deletes whole `perf-*.jsonl` day-files older than `retention_days`;
    /// never the current (or a future) day's file.
    pub(crate) async fn prune(&self, now: SystemTime) -> io::Result<()> {
        let current_day = epoch_day(now);
        let mut read = match fs::read_dir(&self.dir).await {
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        while let Some(entry) = read.next_entry().await? {
            let name = entry.file_name();
            let Some(day) = name.to_str().and_then(perf_day) else {
                continue;
            };
            if day >= current_day {
                continue; // never the current (or a future) day
            }
            if current_day - day >= u64::from(self.retention_days.load(Ordering::Relaxed)) {
                fs::remove_file(entry.path()).await?;
            }
        }
        Ok(())
    }

    fn perf_path(&self, day_epoch: u64) -> PathBuf {
        self.dir
            .join(format!("perf-{}.jsonl", date_string(day_epoch)))
    }
}

/// Day-epoch of a `perf-YYYY-MM-DD.jsonl` filename.
fn perf_day(name: &str) -> Option<u64> {
    name.strip_prefix("perf-")
        .and_then(|s| s.strip_suffix(".jsonl"))
        .and_then(parse_day)
}

/// Days since the Unix epoch for a wall-clock instant.
fn epoch_day(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / SECS_PER_DAY)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use fah_model::{CacheStatsSample, LatencySummary, UpstreamSample};

    use super::*;

    fn retention(days: u32) -> Arc<AtomicU32> {
        Arc::new(AtomicU32::new(days))
    }

    fn at_day(day: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(day * SECS_PER_DAY + 12 * 3600)
    }

    fn sample(ts: u64, qps: f64) -> PerfSample {
        PerfSample {
            ts,
            answers_delta: Default::default(),
            rss_bytes: 55_000_000,
            peak_rss: 123_539_456,
            qps,
            queries_delta: 100,
            blocked_delta: 30,
            allowed_delta: 1,
            cache: CacheStatsSample {
                entries: 1000,
                capacity: 16_384,
                fresh: 900,
                stale: 80,
                expired: 20,
                hits: 5000,
                misses: 1200,
                evictions: 34,
                bytes: 21_000_000,
                max_bytes: 67_108_864,
            },
            latency: LatencySummary {
                block_p50: 0.0001,
                block_p99: 0.0005,
                cache_hit_p50: 0.0001,
                cache_hit_p99: 0.00025,
                forward_p50: 0.005,
                forward_p99: 0.05,
            },
            memory: fah_model::MemoryComponents {
                ruleset: 23_440_198,
                cache: 1_445_728,
                stats: fah_model::StatsHeap::default(),
            },
            minor_page_faults: 231_655,
            rss_anon_bytes: 35_000_000,
            rss_file_bytes: 20_000_000,
            upstreams: vec![UpstreamSample {
                address: "1.1.1.1".to_string(),
                protocol: fah_model::Protocol::Udp,
                attempts: 10,
                failures: 0,
                consecutive_failures: 0,
                tls_handshakes: 0,
                failure_runs: [0, 0, 0, 0],
                state: fah_model::UpstreamState::Healthy,
                penalty_round: 0,
                penalties: 0,
                penalized_seconds_total: 0,
                probes: 0,
                probe_successes: 0,
                family: Some(fah_model::AddressFamily::V4),
            }],
        }
    }

    #[tokio::test]
    async fn append_writes_one_line_per_sample_into_the_day_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = PerfWriter::new(dir.path().to_path_buf(), retention(90));
        writer.boot().await.unwrap();

        // Two samples on the same UTC day (day 818).
        let base = 818 * SECS_PER_DAY;
        writer.append(&sample(base + 60, 1.0)).await.unwrap();
        writer.append(&sample(base + 120, 2.0)).await.unwrap();

        let path = dir.path().join(format!("perf-{}.jsonl", date_string(818)));
        let text = fs::read_to_string(&path).await.unwrap();
        let lines: Vec<PerfSample> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].ts, base + 60);
        assert_eq!(lines[1].qps, 2.0);
    }

    #[tokio::test]
    async fn append_routes_by_the_sample_timestamp_not_wall_clock() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = PerfWriter::new(dir.path().to_path_buf(), retention(90));
        writer.boot().await.unwrap();

        writer
            .append(&sample(818 * SECS_PER_DAY + 60, 1.0))
            .await
            .unwrap();
        writer
            .append(&sample(819 * SECS_PER_DAY + 60, 1.0))
            .await
            .unwrap();

        assert!(dir
            .path()
            .join(format!("perf-{}.jsonl", date_string(818)))
            .exists());
        assert!(dir
            .path()
            .join(format!("perf-{}.jsonl", date_string(819)))
            .exists());
    }

    #[tokio::test]
    async fn prune_deletes_old_day_files_but_keeps_recent_and_current() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = PerfWriter::new(dir.path().to_path_buf(), retention(30));
        writer.boot().await.unwrap();

        let now_day = 20_000u64;
        let old = now_day - 40; // past retention → pruned
        let recent = now_day - 10; // within retention → kept
        writer
            .append(&sample(old * SECS_PER_DAY + 60, 1.0))
            .await
            .unwrap();
        writer
            .append(&sample(recent * SECS_PER_DAY + 60, 1.0))
            .await
            .unwrap();
        writer
            .append(&sample(now_day * SECS_PER_DAY + 60, 1.0))
            .await
            .unwrap();

        writer.prune(at_day(now_day)).await.unwrap();

        assert!(!dir
            .path()
            .join(format!("perf-{}.jsonl", date_string(old)))
            .exists());
        assert!(dir
            .path()
            .join(format!("perf-{}.jsonl", date_string(recent)))
            .exists());
        assert!(dir
            .path()
            .join(format!("perf-{}.jsonl", date_string(now_day)))
            .exists());
    }
}

//! Append-only hourly/daily rollup writer on `/data/history/rollups`
//! (ADR-0002: flat JSONL, no embedded DB). Follows `snapshot.rs`'s atomic
//! tmp-write+rename, keyed by calendar day:
//!
//! - `rollup-YYYY-MM-DD.jsonl` — one [`HourRollup`] line per completed hour
//!   (≤24 lines/day, kilobytes/day).
//! - `top-YYYY-MM-DD.json` — one [`DailyTopN`] object per completed day.
//!
//! All writes run on the flush scheduler, never on the per-query path
//! (hard rule 3). Memory is fixed: the writer holds three cursors, not data.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use fah_model::{DailyTopN, HourRollup};
use tokio::fs;

use crate::bucket::epoch_hour;
use crate::history::append_line;
use crate::history::date::{date_string, parse_day};

const HOURS_PER_DAY: u64 = 24;

/// Fallback prune cadence when no day-file rotation prompts one — often enough
/// that `retention_days` (a storage bound) doesn't lag by more than an hour,
/// rare enough that the directory scan disappears from the steady state.
const PRUNE_INTERVAL: Duration = Duration::from_secs(3600);

pub(crate) struct RollupWriter {
    dir: PathBuf,
    /// Retention held behind an atomic shared with [`Stats`](crate::Stats), so a
    /// `POST /api/v1/config` change applies to the next prune without
    /// reconstructing the writer (hard rule 3: config changes via atomic swap).
    retention_days: Arc<AtomicU32>,
    /// Highest `hour_epoch` already appended — guards against double-counting a
    /// completed hour across ticks and restarts (idempotent boot, hard rule 4:
    /// re-persisting must not grow the file unbounded).
    last_hour_written: Option<u64>,
    /// Highest `day_epoch` whose top-N has been flushed.
    last_day_flushed: Option<u64>,
    last_prune: Option<Instant>,
}

impl RollupWriter {
    pub(crate) fn new(dir: PathBuf, retention_days: Arc<AtomicU32>) -> Self {
        Self {
            dir,
            retention_days,
            last_hour_written: None,
            last_day_flushed: None,
            last_prune: None,
        }
    }

    pub(crate) async fn boot(&mut self) -> io::Result<()> {
        fs::create_dir_all(&self.dir).await?;
        let mut rollups: Vec<(u64, PathBuf)> = Vec::new();
        let mut max_top_day: Option<u64> = None;

        let mut read = fs::read_dir(&self.dir).await?;
        while let Some(entry) = read.next_entry().await? {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Some(day) = name
                .strip_prefix("rollup-")
                .and_then(|s| s.strip_suffix(".jsonl"))
                .and_then(parse_day)
            {
                rollups.push((day, entry.path()));
            } else if let Some(day) = name
                .strip_prefix("top-")
                .and_then(|s| s.strip_suffix(".json"))
                .and_then(parse_day)
            {
                max_top_day = Some(max_top_day.map_or(day, |m| m.max(day)));
            }
        }

        rollups.sort_unstable_by_key(|(day, _)| std::cmp::Reverse(*day));
        self.last_hour_written = None;
        for (_, path) in &rollups {
            let Ok(text) = fs::read_to_string(path).await else {
                continue;
            };
            let newest_hour = text
                .lines()
                .filter_map(|line| serde_json::from_str::<HourRollup>(line).ok())
                .map(|rollup| rollup.hour_epoch)
                .max();
            if newest_hour.is_some() {
                self.last_hour_written = newest_hour;
                break;
            }
        }
        self.last_day_flushed = max_top_day;
        Ok(())
    }

    /// Appends every not-yet-written completed hour (input must be ascending —
    /// `Aggregates::completed_hour_rollups` guarantees it), advancing the
    /// cursor. An hour at or below the cursor is skipped, so a re-run over the
    /// same ring is a no-op.
    pub(crate) async fn append_hours(&mut self, hours: &[HourRollup]) -> io::Result<()> {
        for hour in hours {
            if self
                .last_hour_written
                .is_some_and(|last| hour.hour_epoch <= last)
            {
                continue;
            }
            let line = serde_json::to_string(hour).map_err(io::Error::other)?;
            append_line(&self.rollup_path(hour.hour_epoch), &line).await?;
            self.last_hour_written = Some(hour.hour_epoch);
        }
        Ok(())
    }

    /// The just-completed day whose top-N is due, if any — else `None`. On the
    /// first call it adopts the completed day as a baseline without flushing
    /// (no data is attributable to a day we didn't observe). The caller builds
    /// the [`DailyTopN`] only when this returns `Some`.
    pub(crate) fn day_due(&mut self, now: SystemTime) -> Option<u64> {
        let completed_day = (epoch_hour(now) / HOURS_PER_DAY).checked_sub(1)?;
        match self.last_day_flushed {
            None => {
                self.last_day_flushed = Some(completed_day);
                None
            }
            Some(last) if completed_day > last => Some(completed_day),
            _ => None,
        }
    }

    /// Writes (atomically, overwriting) the completed day's top-N and advances
    /// the day cursor. Overwrite-not-append keeps one object per day-file, so a
    /// retried flush is idempotent.
    pub(crate) async fn append_daily_top(&mut self, top: &DailyTopN) -> io::Result<()> {
        let path = self.top_path(top.day_epoch);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let text = serde_json::to_string(top).map_err(io::Error::other)?;

        let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
        tmp_name.push(format!(".tmp.{}", std::process::id()));
        let tmp = path.with_file_name(tmp_name);
        fs::write(&tmp, text).await?;
        fs::rename(&tmp, &path).await?;

        self.last_day_flushed = Some(top.day_epoch);
        Ok(())
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

    /// Deletes whole day-files (both `rollup-` and `top-`) older than
    /// `retention_days`; never the current day's file. Rollups are tiny, so
    /// age is the only bound here — no byte cap.
    pub(crate) async fn prune(&self, now: SystemTime) -> io::Result<()> {
        let current_day = epoch_hour(now) / HOURS_PER_DAY;
        let mut read = match fs::read_dir(&self.dir).await {
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        while let Some(entry) = read.next_entry().await? {
            let name = entry.file_name();
            let Some(day) = name.to_str().and_then(history_day) else {
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

    fn rollup_path(&self, hour_epoch: u64) -> PathBuf {
        self.dir.join(format!(
            "rollup-{}.jsonl",
            date_string(hour_epoch / HOURS_PER_DAY)
        ))
    }

    fn top_path(&self, day_epoch: u64) -> PathBuf {
        self.dir
            .join(format!("top-{}.json", date_string(day_epoch)))
    }
}

/// Day-epoch of a `rollup-YYYY-MM-DD.jsonl` or `top-YYYY-MM-DD.json` filename.
fn history_day(name: &str) -> Option<u64> {
    name.strip_prefix("rollup-")
        .and_then(|s| s.strip_suffix(".jsonl"))
        .or_else(|| {
            name.strip_prefix("top-")
                .and_then(|s| s.strip_suffix(".json"))
        })
        .and_then(parse_day)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::path::Path;
    use std::time::{Duration, UNIX_EPOCH};

    use fah_model::{ClientHits, DomainHits};

    use super::*;

    fn retention(days: u32) -> Arc<AtomicU32> {
        Arc::new(AtomicU32::new(days))
    }

    fn at_hour(hour: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(hour * 3600)
    }

    fn hour_rollup(hour_epoch: u64, queries: u64) -> HourRollup {
        HourRollup {
            hour_epoch,
            queries,
            blocked: 0,
            cache_hits: 0,
            per_type: std::collections::BTreeMap::from([("A".to_string(), queries)]),
        }
    }

    async fn read_lines(path: &Path) -> Vec<HourRollup> {
        fs::read_to_string(path)
            .await
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn append_hours_writes_one_line_per_hour_into_the_day_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = RollupWriter::new(dir.path().to_path_buf(), retention(90));
        writer.boot().await.unwrap();

        // Two hours on the same UTC day (day 818).
        let base = 818 * HOURS_PER_DAY;
        writer
            .append_hours(&[hour_rollup(base + 1, 10), hour_rollup(base + 2, 20)])
            .await
            .unwrap();

        let path = dir
            .path()
            .join(format!("rollup-{}.jsonl", date_string(818)));
        let lines = read_lines(&path).await;
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].hour_epoch, base + 1);
        assert_eq!(lines[1].queries, 20);
        assert_eq!(writer.last_hour_written, Some(base + 2));
    }

    #[tokio::test]
    async fn append_hours_is_idempotent_for_already_written_hours() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = RollupWriter::new(dir.path().to_path_buf(), retention(90));
        writer.boot().await.unwrap();

        let base = 818 * HOURS_PER_DAY;
        writer
            .append_hours(&[hour_rollup(base + 1, 10)])
            .await
            .unwrap();
        // Same completed hour offered again (plus a genuinely new one).
        writer
            .append_hours(&[hour_rollup(base + 1, 10), hour_rollup(base + 2, 20)])
            .await
            .unwrap();

        let path = dir
            .path()
            .join(format!("rollup-{}.jsonl", date_string(818)));
        let lines = read_lines(&path).await;
        assert_eq!(
            lines.len(),
            2,
            "the duplicated hour must not be re-appended"
        );
    }

    #[tokio::test]
    async fn boot_resumes_the_hour_cursor_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let base = 818 * HOURS_PER_DAY;
        {
            let mut writer = RollupWriter::new(dir.path().to_path_buf(), retention(90));
            writer.boot().await.unwrap();
            writer
                .append_hours(&[hour_rollup(base + 1, 10), hour_rollup(base + 2, 20)])
                .await
                .unwrap();
        }

        let mut restarted = RollupWriter::new(dir.path().to_path_buf(), retention(90));
        restarted.boot().await.unwrap();
        assert_eq!(restarted.last_hour_written, Some(base + 2));

        // Re-offering the persisted hours after restart appends nothing.
        restarted
            .append_hours(&[hour_rollup(base + 1, 10), hour_rollup(base + 2, 20)])
            .await
            .unwrap();
        let lines = read_lines(
            &dir.path()
                .join(format!("rollup-{}.jsonl", date_string(818))),
        )
        .await;
        assert_eq!(lines.len(), 2);
    }

    #[tokio::test]
    async fn boot_walks_past_an_empty_newest_day_file_to_the_last_written_hour() {
        let dir = tempfile::tempdir().unwrap();
        let base = 818 * HOURS_PER_DAY;
        {
            let mut writer = RollupWriter::new(dir.path().to_path_buf(), retention(90));
            writer.boot().await.unwrap();
            writer
                .append_hours(&[hour_rollup(base + 22, 10), hour_rollup(base + 23, 20)])
                .await
                .unwrap();
        }
        fs::write(
            dir.path()
                .join(format!("rollup-{}.jsonl", date_string(819))),
            "",
        )
        .await
        .unwrap();

        let mut restarted = RollupWriter::new(dir.path().to_path_buf(), retention(90));
        restarted.boot().await.unwrap();
        assert_eq!(restarted.last_hour_written, Some(base + 23));

        restarted
            .append_hours(&[hour_rollup(base + 22, 10), hour_rollup(base + 23, 20)])
            .await
            .unwrap();
        let lines = read_lines(
            &dir.path()
                .join(format!("rollup-{}.jsonl", date_string(818))),
        )
        .await;
        assert_eq!(
            lines.len(),
            2,
            "an empty newest file must not reset the cursor"
        );
    }

    #[tokio::test]
    async fn boot_reads_a_row_that_carries_no_per_type() {
        let dir = tempfile::tempdir().unwrap();
        let base = 818 * HOURS_PER_DAY;
        fs::write(
            dir.path()
                .join(format!("rollup-{}.jsonl", date_string(818))),
            format!(
                "{{\"hour_epoch\":{},\"queries\":1,\"blocked\":0,\"cache_hits\":0}}\n",
                base + 5
            ),
        )
        .await
        .unwrap();

        let mut writer = RollupWriter::new(dir.path().to_path_buf(), retention(90));
        writer.boot().await.unwrap();
        assert_eq!(writer.last_hour_written, Some(base + 5));
    }

    #[tokio::test]
    async fn day_due_baselines_then_fires_once_per_completed_day() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = RollupWriter::new(dir.path().to_path_buf(), retention(90));
        writer.boot().await.unwrap();

        // First observation on day 818: baseline, no flush.
        assert_eq!(writer.day_due(at_hour(818 * HOURS_PER_DAY + 5)), None);
        // Still day 818: nothing due.
        assert_eq!(writer.day_due(at_hour(818 * HOURS_PER_DAY + 23)), None);
        // Rolled into day 819: day 818 just completed.
        assert_eq!(writer.day_due(at_hour(819 * HOURS_PER_DAY + 1)), Some(818));
    }

    #[tokio::test]
    async fn append_daily_top_writes_one_object_and_advances_the_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = RollupWriter::new(dir.path().to_path_buf(), retention(90));
        writer.boot().await.unwrap();

        let top = DailyTopN {
            day_epoch: 818,
            top_blocked: vec![DomainHits {
                domain: "ads.example.com".to_string(),
                count: 5,
            }],
            top_queried: vec![],
            top_clients: vec![ClientHits {
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                name: None,
                count: 9,
            }],
        };
        writer.append_daily_top(&top).await.unwrap();

        let path = dir.path().join(format!("top-{}.json", date_string(818)));
        let back: DailyTopN =
            serde_json::from_str(&fs::read_to_string(&path).await.unwrap()).unwrap();
        assert_eq!(back, top);
        assert_eq!(writer.last_day_flushed, Some(818));
    }

    #[tokio::test]
    async fn prune_deletes_day_files_past_retention_but_keeps_recent_ones() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = RollupWriter::new(dir.path().to_path_buf(), retention(30));
        writer.boot().await.unwrap();

        let now_day = 20_000u64;
        // 40 days old → pruned; 10 days old → kept.
        let old = now_day - 40;
        let recent = now_day - 10;
        writer
            .append_hours(&[hour_rollup(old * HOURS_PER_DAY, 1)])
            .await
            .unwrap();
        writer
            .append_hours(&[hour_rollup(recent * HOURS_PER_DAY, 1)])
            .await
            .unwrap();

        writer
            .prune(at_hour(now_day * HOURS_PER_DAY))
            .await
            .unwrap();

        assert!(!dir
            .path()
            .join(format!("rollup-{}.jsonl", date_string(old)))
            .exists());
        assert!(dir
            .path()
            .join(format!("rollup-{}.jsonl", date_string(recent)))
            .exists());
    }

    #[tokio::test]
    async fn prune_never_deletes_the_current_day() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = RollupWriter::new(dir.path().to_path_buf(), retention(1));
        writer.boot().await.unwrap();

        let now_day = 20_000u64;
        writer
            .append_hours(&[hour_rollup(now_day * HOURS_PER_DAY, 1)])
            .await
            .unwrap();
        writer
            .prune(at_hour(now_day * HOURS_PER_DAY + 5))
            .await
            .unwrap();

        assert!(dir
            .path()
            .join(format!("rollup-{}.jsonl", date_string(now_day)))
            .exists());
    }

    #[tokio::test]
    async fn live_retention_change_moves_the_next_prune_cutoff() {
        let dir = tempfile::tempdir().unwrap();
        // The same atomic `Stats` shares with both writers; a POST
        // /api/v1/config stores the new value into it.
        let days = retention(90);
        let mut writer = RollupWriter::new(dir.path().to_path_buf(), Arc::clone(&days));
        writer.boot().await.unwrap();

        let now_day = 20_000u64;
        let old = now_day - 40; // 40 days old
        writer
            .append_hours(&[hour_rollup(old * HOURS_PER_DAY, 1)])
            .await
            .unwrap();
        let path = dir
            .path()
            .join(format!("rollup-{}.jsonl", date_string(old)));

        // Under a 90-day window the 40-day-old file survives.
        writer
            .prune(at_hour(now_day * HOURS_PER_DAY))
            .await
            .unwrap();
        assert!(path.exists(), "kept under the wide window");

        // Tighten retention live — no reconstruction, just the shared atomic.
        days.store(30, Ordering::Relaxed);
        writer
            .prune(at_hour(now_day * HOURS_PER_DAY))
            .await
            .unwrap();
        assert!(
            !path.exists(),
            "the very next prune honors the tightened window"
        );
    }
}

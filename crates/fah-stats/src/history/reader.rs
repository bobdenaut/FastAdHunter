//! Range-scoped reads of what the history writers persisted — the data behind
//! `GET /api/v1/history/{summary,perf,top}`.
//!
//! Two properties this module is built around:
//!
//! - **Bounded.** A read never loads a range into memory to shrink it
//!   afterwards. Only the day-files whose calendar day intersects the range are
//!   opened, each is streamed line by line, and the result is capped as it is
//!   built ([`Decimator`]) — so 90 days costs the same memory as one hour
//!   (hard rule 4).
//! - **Blocking.** Deliberately `std::fs`, not `tokio::fs`: a multi-day read is
//!   one `spawn_blocking` hop for the whole scan instead of one per file (which
//!   is what `tokio::fs` is, internally). The caller — the API handler — owns
//!   that hop. Nothing here runs on the per-query path.
//!
//! A line that no longer parses is skipped rather than failing the range (a
//! torn final line after a power cut, or a row from an older schema) — the same
//! policy the writers' `boot` already uses.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fah_model::{
    ClientHits, DailyTopN, DomainHits, HistoryPoint, HistoryRange, HistoryResolution,
    HistorySeries, HourRollup, PerfSample, PerfSeries, TopItems, TopKind,
};

const SECONDS_PER_HOUR: u64 = 3_600;
const SECONDS_PER_DAY: u64 = 86_400;

/// Reads the two history stores `Stats` writes: the rollup/top day-files and
/// the perf sample day-files. Holds paths only — no cursors, no cache, so a
/// read never disagrees with what is on disk.
pub(crate) struct HistoryReader {
    rollups: PathBuf,
    perf: PathBuf,
}

impl HistoryReader {
    pub(crate) fn new(rollups: PathBuf, perf: PathBuf) -> Self {
        Self { rollups, perf }
    }

    /// Hourly (or daily) aggregates within the range. At [`HistoryResolution::Day`]
    /// the day's hourly rows are summed — the day-files are the only stored
    /// resolution, so a daily chart is derived here rather than persisted twice.
    pub(crate) fn summary(
        &self,
        range: HistoryRange,
        resolution: HistoryResolution,
        max_points: usize,
    ) -> io::Result<HistorySeries> {
        let Some((from, to)) = window(range) else {
            return Ok(HistorySeries {
                points: Vec::new(),
                stride: 1,
            });
        };
        let files = day_files(&self.rollups, "rollup-", ".jsonl", from, to)?;

        match resolution {
            HistoryResolution::Hour => {
                let mut points = Decimator::new(max_points);
                for path in &files {
                    for_each_row(path, |rollup: HourRollup| {
                        let ts = rollup.hour_epoch.saturating_mul(SECONDS_PER_HOUR);
                        if (from..to).contains(&ts) {
                            points.push(point(ts, rollup));
                        }
                    })?;
                }
                let (points, stride) = points.finish();
                Ok(HistorySeries { points, stride })
            }
            HistoryResolution::Day => {
                // Keyed by day-start, so the map is bounded by the day-files
                // retention left on disk — not by how wide a range was asked
                // for. `BTreeMap` also hands the days back ascending.
                let mut days: BTreeMap<u64, HistoryPoint> = BTreeMap::new();
                for path in &files {
                    for_each_row(path, |rollup: HourRollup| {
                        let ts = rollup.hour_epoch.saturating_mul(SECONDS_PER_HOUR);
                        if !(from..to).contains(&ts) {
                            return;
                        }
                        let day_start = ts / SECONDS_PER_DAY * SECONDS_PER_DAY;
                        let day = days.entry(day_start).or_insert_with(|| HistoryPoint {
                            ts: day_start,
                            queries: 0,
                            blocked: 0,
                            cache_hits: 0,
                            per_type: BTreeMap::new(),
                        });
                        day.queries += rollup.queries;
                        day.blocked += rollup.blocked;
                        day.cache_hits += rollup.cache_hits;
                        for (qtype, count) in rollup.per_type {
                            *day.per_type.entry(qtype).or_insert(0) += count;
                        }
                    })?;
                }
                let mut points = Decimator::new(max_points);
                for day in days.into_values() {
                    points.push(day);
                }
                let (points, stride) = points.finish();
                Ok(HistorySeries { points, stride })
            }
        }
    }

    /// The perf sample series within the range, decimated to `max_points`.
    pub(crate) fn perf(&self, range: HistoryRange, max_points: usize) -> io::Result<PerfSeries> {
        let Some((from, to)) = window(range) else {
            return Ok(PerfSeries {
                samples: Vec::new(),
                stride: 1,
            });
        };
        let files = day_files(&self.perf, "perf-", ".jsonl", from, to)?;

        let mut samples = Decimator::new(max_points);
        for path in &files {
            for_each_row(path, |sample: PerfSample| {
                if (from..to).contains(&sample.ts) {
                    samples.push(sample);
                }
            })?;
        }
        let (samples, stride) = samples.finish();
        Ok(PerfSeries { samples, stride })
    }

    /// Top-N over the range, merged from the daily top-N files by summing each
    /// key's counts.
    ///
    /// **This is an estimate of an estimate.** Each day-file already holds only
    /// that day's top-N (a space-saving approximation), so a domain that missed
    /// the cut on some day contributes nothing for it — a domain steadily just
    /// below the daily cut-off can rank below one that spiked into a single
    /// day's top-N. Fine for "what dominated this week"; not an exact ranking,
    /// and documented as such wherever it is served.
    pub(crate) fn top(
        &self,
        range: HistoryRange,
        kind: TopKind,
        limit: usize,
    ) -> io::Result<TopItems> {
        let Some((from, to)) = window(range) else {
            return Ok(empty_top(kind));
        };
        let files = day_files(&self.rollups, "top-", ".json", from, to)?;

        // Each file holds at most a handful of entries per category, so the
        // merge is bounded by (entries × retained days) — kilobytes.
        let mut domains: HashMap<String, u64> = HashMap::new();
        let mut clients: HashMap<IpAddr, ClientHits> = HashMap::new();

        for path in &files {
            let Some(top) = read_object::<DailyTopN>(path)? else {
                continue;
            };
            let domain_hits = match kind {
                TopKind::Blocked => top.top_blocked,
                TopKind::Queried => top.top_queried,
                TopKind::Clients => {
                    for hit in top.top_clients {
                        let entry = clients.entry(hit.ip).or_insert_with(|| ClientHits {
                            ip: hit.ip,
                            name: None,
                            count: 0,
                        });
                        entry.count += hit.count;
                        // Files are merged oldest-first, so the newest name a
                        // client was recorded under wins a rename.
                        if hit.name.is_some() {
                            entry.name = hit.name;
                        }
                    }
                    continue;
                }
            };
            for hit in domain_hits {
                *domains.entry(hit.domain).or_insert(0) += hit.count;
            }
        }

        Ok(match kind {
            TopKind::Clients => {
                let mut items: Vec<ClientHits> = clients.into_values().collect();
                // Count descending; the key breaks ties so the ranking is
                // stable across calls rather than following hash order.
                items.sort_unstable_by(|a, b| b.count.cmp(&a.count).then_with(|| a.ip.cmp(&b.ip)));
                items.truncate(limit);
                TopItems::Clients(items)
            }
            _ => {
                let mut items: Vec<DomainHits> = domains
                    .into_iter()
                    .map(|(domain, count)| DomainHits { domain, count })
                    .collect();
                items.sort_unstable_by(|a, b| {
                    b.count.cmp(&a.count).then_with(|| a.domain.cmp(&b.domain))
                });
                items.truncate(limit);
                TopItems::Domains(items)
            }
        })
    }
}

/// The range as `[from, to)` in epoch seconds, or `None` when it is empty or
/// inverted — every read then answers with an empty series rather than
/// guessing what was meant.
fn window(range: HistoryRange) -> Option<(u64, u64)> {
    let from = epoch_seconds(range.from);
    let to = epoch_seconds(range.to);
    (to > from).then_some((from, to))
}

fn epoch_seconds(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn point(ts: u64, rollup: HourRollup) -> HistoryPoint {
    HistoryPoint {
        ts,
        queries: rollup.queries,
        blocked: rollup.blocked,
        cache_hits: rollup.cache_hits,
        per_type: rollup.per_type,
    }
}

fn empty_top(kind: TopKind) -> TopItems {
    match kind {
        TopKind::Clients => TopItems::Clients(Vec::new()),
        _ => TopItems::Domains(Vec::new()),
    }
}

/// The `{prefix}YYYY-MM-DD{suffix}` files whose calendar day intersects
/// `[from, to)`, ascending. Only existing files are considered, so the work a
/// read does is bounded by what retention left on disk — a caller asking for
/// the year 1970 scans nothing.
fn day_files(
    dir: &Path,
    prefix: &str,
    suffix: &str,
    from: u64,
    to: u64,
) -> io::Result<Vec<PathBuf>> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        // Nothing written yet is an empty series, not a failure.
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };

    let first_day = from / SECONDS_PER_DAY;
    let last_day = (to - 1) / SECONDS_PER_DAY; // `to` is exclusive
    let mut files: Vec<(u64, PathBuf)> = Vec::new();
    for entry in read {
        let entry = entry?;
        let name = entry.file_name();
        let Some(day) = name
            .to_str()
            .and_then(|name| name.strip_prefix(prefix))
            .and_then(|name| name.strip_suffix(suffix))
            .and_then(crate::history::date::parse_day)
        else {
            continue;
        };
        if (first_day..=last_day).contains(&day) {
            files.push((day, entry.path()));
        }
    }
    files.sort_unstable_by_key(|(day, _)| *day);
    Ok(files.into_iter().map(|(_, path)| path).collect())
}

/// Streams a JSONL file, handing each parsed row to `visit`. Unparseable lines
/// are skipped, a missing file is empty (it can be pruned between the directory
/// listing and the open), and I/O errors propagate.
fn for_each_row<T: serde::de::DeserializeOwned>(
    path: &Path,
    mut visit: impl FnMut(T),
) -> io::Result<()> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for line in BufReader::new(file).lines() {
        if let Ok(row) = serde_json::from_str::<T>(&line?) {
            visit(row);
        }
    }
    Ok(())
}

/// Reads one whole-file JSON object (the `top-YYYY-MM-DD.json` shape).
/// `Ok(None)` for a missing or unparseable file — one bad day must not fail a
/// week-long range.
fn read_object<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text).ok()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Keeps at most `max` items out of an arbitrarily long stream, in one pass:
/// every `stride`-th item is kept, and whenever the buffer would exceed `max`
/// it is halved and the stride doubled. Memory is bounded by `max` however many
/// rows the range covers (hard rule 4), and every item that survives is a
/// verbatim row — decimation makes a series sparser, it never averages a spike
/// away. The final `stride` is reported to the caller so a chart can say what
/// it is showing.
struct Decimator<T> {
    kept: Vec<T>,
    seen: u64,
    stride: u64,
    max: usize,
}

impl<T> Decimator<T> {
    fn new(max: usize) -> Self {
        Self {
            kept: Vec::new(),
            seen: 0,
            stride: 1,
            max: max.max(1),
        }
    }

    fn push(&mut self, item: T) {
        if self.seen.is_multiple_of(self.stride) {
            self.kept.push(item);
        }
        self.seen += 1;
        if self.kept.len() > self.max {
            // Halving keeps positions 0, 2, 4, … — whose original indices are
            // multiples of `2 × stride`, exactly what the doubled stride goes
            // on selecting. The kept series stays evenly spaced across the halving.
            let mut index = 0;
            self.kept.retain(|_| {
                let keep = index % 2 == 0;
                index += 1;
                keep
            });
            self.stride *= 2;
        }
    }

    fn finish(self) -> (Vec<T>, u64) {
        (self.kept, self.stride)
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use fah_model::{CacheStatsSample, LatencySummary};

    use super::*;
    use crate::history::date::date_string;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn range(from: u64, to: u64) -> HistoryRange {
        HistoryRange {
            from: at(from),
            to: at(to),
        }
    }

    /// Day 19625 = 2023-09-25 — the day the writer tests already use.
    const DAY: u64 = 19_625;

    fn reader(dir: &Path) -> HistoryReader {
        HistoryReader::new(dir.join("rollups"), dir.join("perf"))
    }

    fn write_rollups(dir: &Path, day: u64, rows: &[HourRollup]) {
        let path = dir.join("rollups");
        std::fs::create_dir_all(&path).unwrap();
        let text: String = rows
            .iter()
            .map(|row| format!("{}\n", serde_json::to_string(row).unwrap()))
            .collect();
        std::fs::write(
            path.join(format!("rollup-{}.jsonl", date_string(day))),
            text,
        )
        .unwrap();
    }

    fn hour(day: u64, hour_of_day: u64, queries: u64, blocked: u64) -> HourRollup {
        HourRollup {
            hour_epoch: day * 24 + hour_of_day,
            queries,
            blocked,
            cache_hits: queries / 2,
            per_type: BTreeMap::from([("A".to_string(), queries)]),
        }
    }

    fn sample(ts: u64) -> PerfSample {
        PerfSample {
            ts,
            answers_delta: Default::default(),
            rss_bytes: 55_000_000,
            peak_rss: 123_539_456,
            qps: 1.0,
            queries_delta: 60,
            blocked_delta: 10,
            allowed_delta: 0,
            cache: CacheStatsSample {
                entries: 1,
                capacity: 2,
                fresh: 1,
                stale: 0,
                expired: 0,
                hits: 1,
                misses: 1,
                evictions: 0,
                bytes: 1024,
                max_bytes: 67_108_864,
            },
            latency: LatencySummary {
                block_p50: 0.0,
                block_p99: 0.0,
                cache_hit_p50: 0.0,
                cache_hit_p99: 0.0,
                forward_p50: 0.0,
                forward_p99: 0.0,
            },
            memory: fah_model::MemoryComponents::default(),
            minor_page_faults: 0,
            rss_anon_bytes: 0,
            rss_file_bytes: 0,
            upstreams: vec![],
        }
    }

    fn write_samples(dir: &Path, day: u64, samples: &[PerfSample]) {
        let path = dir.join("perf");
        std::fs::create_dir_all(&path).unwrap();
        let text: String = samples
            .iter()
            .map(|row| format!("{}\n", serde_json::to_string(row).unwrap()))
            .collect();
        std::fs::write(path.join(format!("perf-{}.jsonl", date_string(day))), text).unwrap();
    }

    fn write_top(dir: &Path, day: u64, top: &DailyTopN) {
        let path = dir.join("rollups");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join(format!("top-{}.json", date_string(day))),
            serde_json::to_string(top).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn hour_resolution_returns_one_point_per_stored_hour_in_range() {
        let dir = tempfile::tempdir().unwrap();
        write_rollups(
            dir.path(),
            DAY,
            &[hour(DAY, 1, 100, 10), hour(DAY, 2, 200, 20)],
        );

        let day_start = DAY * SECONDS_PER_DAY;
        let series = reader(dir.path())
            .summary(
                range(day_start, day_start + SECONDS_PER_DAY),
                HistoryResolution::Hour,
                5_000,
            )
            .unwrap();

        assert_eq!(series.stride, 1);
        assert_eq!(series.points.len(), 2);
        assert_eq!(series.points[0].ts, day_start + SECONDS_PER_HOUR);
        assert_eq!(series.points[0].queries, 100);
        assert_eq!(series.points[1].blocked, 20);
        assert_eq!(series.points[1].per_type.get("A"), Some(&200));
    }

    #[test]
    fn day_resolution_sums_the_hours_of_each_day() {
        let dir = tempfile::tempdir().unwrap();
        write_rollups(
            dir.path(),
            DAY,
            &[hour(DAY, 1, 100, 10), hour(DAY, 2, 200, 20)],
        );
        write_rollups(dir.path(), DAY + 1, &[hour(DAY + 1, 5, 50, 5)]);

        let series = reader(dir.path())
            .summary(
                range(DAY * SECONDS_PER_DAY, (DAY + 2) * SECONDS_PER_DAY),
                HistoryResolution::Day,
                5_000,
            )
            .unwrap();

        assert_eq!(series.points.len(), 2, "one point per day, ascending");
        assert_eq!(series.points[0].ts, DAY * SECONDS_PER_DAY);
        assert_eq!(series.points[0].queries, 300);
        assert_eq!(series.points[0].blocked, 30);
        assert_eq!(series.points[0].cache_hits, 150);
        assert_eq!(
            series.points[0].per_type.get("A"),
            Some(&300),
            "per-type counts merge across the day's hours"
        );
        assert_eq!(series.points[1].queries, 50);
    }

    #[test]
    fn the_range_is_half_open_and_excludes_hours_outside_it() {
        let dir = tempfile::tempdir().unwrap();
        write_rollups(
            dir.path(),
            DAY,
            &[hour(DAY, 0, 1, 0), hour(DAY, 1, 2, 0), hour(DAY, 2, 3, 0)],
        );

        let day_start = DAY * SECONDS_PER_DAY;
        // [01:00, 02:00) — the middle hour only.
        let series = reader(dir.path())
            .summary(
                range(
                    day_start + SECONDS_PER_HOUR,
                    day_start + 2 * SECONDS_PER_HOUR,
                ),
                HistoryResolution::Hour,
                5_000,
            )
            .unwrap();

        assert_eq!(series.points.len(), 1);
        assert_eq!(series.points[0].queries, 2);
    }

    #[test]
    fn a_range_with_no_data_is_an_empty_series_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let reader = reader(dir.path());

        // Nothing written at all — the directories do not even exist.
        let series = reader
            .summary(range(0, SECONDS_PER_DAY), HistoryResolution::Hour, 100)
            .unwrap();
        assert!(series.points.is_empty());
        let perf = reader.perf(range(0, SECONDS_PER_DAY), 100).unwrap();
        assert!(perf.samples.is_empty());
        assert_eq!(
            reader
                .top(range(0, SECONDS_PER_DAY), TopKind::Blocked, 10)
                .unwrap(),
            TopItems::Domains(vec![])
        );

        // A stored day, queried far away from it.
        write_rollups(dir.path(), DAY, &[hour(DAY, 1, 100, 10)]);
        let series = reader
            .summary(range(0, SECONDS_PER_DAY), HistoryResolution::Hour, 100)
            .unwrap();
        assert!(series.points.is_empty());
    }

    #[test]
    fn an_inverted_or_empty_range_reads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        write_rollups(dir.path(), DAY, &[hour(DAY, 1, 100, 10)]);
        let reader = reader(dir.path());
        let start = DAY * SECONDS_PER_DAY;

        for range in [range(start + 10, start), range(start, start)] {
            let series = reader.summary(range, HistoryResolution::Hour, 100).unwrap();
            assert!(series.points.is_empty());
        }
    }

    #[test]
    fn perf_samples_are_read_across_day_files_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let first = DAY * SECONDS_PER_DAY;
        let second = (DAY + 1) * SECONDS_PER_DAY;
        write_samples(dir.path(), DAY, &[sample(first + 60), sample(first + 120)]);
        write_samples(dir.path(), DAY + 1, &[sample(second + 60)]);

        let series = reader(dir.path())
            .perf(range(first, second + SECONDS_PER_DAY), 5_000)
            .unwrap();

        assert_eq!(series.stride, 1);
        let timestamps: Vec<u64> = series.samples.iter().map(|s| s.ts).collect();
        assert_eq!(timestamps, [first + 60, first + 120, second + 60]);
    }

    #[test]
    fn a_wide_perf_range_is_decimated_to_the_point_budget() {
        let dir = tempfile::tempdir().unwrap();
        let start = DAY * SECONDS_PER_DAY;
        // A full day of 60 s samples: 1440 rows.
        let samples: Vec<PerfSample> = (0..1_440).map(|i| sample(start + i * 60)).collect();
        write_samples(dir.path(), DAY, &samples);

        let series = reader(dir.path())
            .perf(range(start, start + SECONDS_PER_DAY), 100)
            .unwrap();

        assert!(
            series.samples.len() <= 100,
            "the budget bounds the response: got {}",
            series.samples.len()
        );
        assert!(series.stride > 1, "and the caller is told it was decimated");
        // Evenly spaced, and every retained row is a verbatim reading.
        let step = series.samples[1].ts - series.samples[0].ts;
        assert_eq!(step, series.stride * 60);
        for pair in series.samples.windows(2) {
            assert_eq!(pair[1].ts - pair[0].ts, step);
        }
    }

    #[test]
    fn top_merges_counts_across_the_days_in_range() {
        let dir = tempfile::tempdir().unwrap();
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        write_top(
            dir.path(),
            DAY,
            &DailyTopN {
                day_epoch: DAY,
                top_blocked: vec![
                    DomainHits {
                        domain: "ads.example.com".to_string(),
                        count: 10,
                    },
                    DomainHits {
                        domain: "tracker.example.com".to_string(),
                        count: 30,
                    },
                ],
                top_queried: vec![],
                top_clients: vec![ClientHits {
                    ip,
                    name: None,
                    count: 5,
                }],
            },
        );
        write_top(
            dir.path(),
            DAY + 1,
            &DailyTopN {
                day_epoch: DAY + 1,
                top_blocked: vec![DomainHits {
                    domain: "ads.example.com".to_string(),
                    count: 40,
                }],
                top_queried: vec![],
                top_clients: vec![ClientHits {
                    ip,
                    name: Some("liviu-phone".to_string()),
                    count: 7,
                }],
            },
        );

        let reader = reader(dir.path());
        let whole = range(DAY * SECONDS_PER_DAY, (DAY + 2) * SECONDS_PER_DAY);

        let TopItems::Domains(domains) = reader.top(whole, TopKind::Blocked, 10).unwrap() else {
            panic!("blocked ranks domains");
        };
        assert_eq!(domains[0].domain, "ads.example.com");
        assert_eq!(domains[0].count, 50, "10 + 40 across the two days");
        assert_eq!(domains[1].count, 30);

        let TopItems::Clients(clients) = reader.top(whole, TopKind::Clients, 10).unwrap() else {
            panic!("clients rank by IP");
        };
        assert_eq!(clients[0].count, 12);
        assert_eq!(
            clients[0].name.as_deref(),
            Some("liviu-phone"),
            "the newest name a client was recorded under wins"
        );

        // Only the first day: the second day's counts are not merged in.
        let TopItems::Domains(domains) = reader
            .top(
                range(DAY * SECONDS_PER_DAY, (DAY + 1) * SECONDS_PER_DAY),
                TopKind::Blocked,
                10,
            )
            .unwrap()
        else {
            panic!("blocked ranks domains");
        };
        assert_eq!(domains[0].domain, "tracker.example.com");
        assert_eq!(domains[0].count, 30);
    }

    #[test]
    fn top_honors_the_requested_n() {
        let dir = tempfile::tempdir().unwrap();
        write_top(
            dir.path(),
            DAY,
            &DailyTopN {
                day_epoch: DAY,
                top_blocked: (0..10)
                    .map(|i| DomainHits {
                        domain: format!("d{i}.example.com"),
                        count: i,
                    })
                    .collect(),
                top_queried: vec![],
                top_clients: vec![],
            },
        );

        let TopItems::Domains(domains) = reader(dir.path())
            .top(
                range(DAY * SECONDS_PER_DAY, (DAY + 1) * SECONDS_PER_DAY),
                TopKind::Blocked,
                3,
            )
            .unwrap()
        else {
            panic!("blocked ranks domains");
        };
        assert_eq!(domains.len(), 3);
        assert_eq!(domains[0].count, 9, "highest first");
    }

    #[test]
    fn an_unparseable_line_is_skipped_rather_than_failing_the_range() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollups");
        std::fs::create_dir_all(&path).unwrap();
        let good = serde_json::to_string(&hour(DAY, 1, 100, 10)).unwrap();
        // A torn final line, as a power cut mid-append leaves one.
        std::fs::write(
            path.join(format!("rollup-{}.jsonl", date_string(DAY))),
            format!("{good}\n{{\"hour_epoch\": 4\n"),
        )
        .unwrap();

        let series = reader(dir.path())
            .summary(
                range(DAY * SECONDS_PER_DAY, (DAY + 1) * SECONDS_PER_DAY),
                HistoryResolution::Hour,
                100,
            )
            .unwrap();
        assert_eq!(series.points.len(), 1);
        assert_eq!(series.points[0].queries, 100);
    }

    /// The other half of the budget contract: a series *under* the budget must
    /// come back whole, at stride 1. The TUI monitor asks for 5000 points so a
    /// day of 60 s samples is never thinned — and a half-open 24 h window holds
    /// 1440 or 1441 of them depending on the sampler's phase, so both counts
    /// have to clear it or the graph flips resolution between polls.
    #[test]
    fn a_series_under_the_budget_is_not_decimated_at_all() {
        for count in [1_440u64, 1_441] {
            let mut decimator = Decimator::new(5_000);
            for i in 0..count {
                decimator.push(i);
            }
            let (kept, stride) = decimator.finish();

            assert_eq!(stride, 1, "{count} samples must arrive undecimated");
            assert_eq!(kept.len() as u64, count, "every row survives");
        }
    }

    #[test]
    fn decimator_keeps_an_evenly_spaced_sample_within_its_budget() {
        for (count, max) in [(10u64, 100usize), (101, 100), (1_000, 10), (100_000, 7)] {
            let mut decimator = Decimator::new(max);
            for i in 0..count {
                decimator.push(i);
            }
            let (kept, stride) = decimator.finish();
            assert!(kept.len() <= max, "budget honored for {count}/{max}");
            assert!(!kept.is_empty());
            // Every kept item sits on a stride boundary, evenly spaced.
            for (position, item) in kept.iter().enumerate() {
                assert_eq!(*item, position as u64 * stride);
            }
        }
    }
}

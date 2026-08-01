//! Batched append-only JSONL segments on `/data` (ADR-0002: flat files, no
//! embedded DB). One [`QueryLogEntry`] per line; segments rotate at
//! [`MAX_SEGMENT_BYTES`] so age/size retention has fine-enough granularity,
//! and [`SegmentWriter::prune`] deletes the oldest first — by age
//! (`retention_days`) and total size (`retention_max_mb`), never touching the
//! segment currently being appended to.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use tokio::fs;
use tokio::io::AsyncWriteExt;

use super::QueryLogEntry;

/// Keeps individual segment files small enough that pruning doesn't have to
/// wait for one giant file to age out all at once.
const MAX_SEGMENT_BYTES: u64 = 1024 * 1024;

/// Fallback cadence for [`SegmentWriter::maybe_prune`] when no rotation
/// happens (low traffic): often enough that the age cap (a privacy control)
/// doesn't lag by more than an hour, rare enough that the per-flush
/// directory scan disappears from the steady state.
const PRUNE_INTERVAL: Duration = Duration::from_secs(3600);

pub(crate) struct SegmentWriter {
    dir: PathBuf,
    current_path: Option<PathBuf>,
    current_size: u64,
    next_index: u64,
    last_prune: Option<Instant>,
}

impl SegmentWriter {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            current_path: None,
            current_size: 0,
            next_index: 0,
            last_prune: None,
        }
    }

    /// Scans `dir` for existing segments so a restart resumes numbering
    /// instead of overwriting the previous run's files.
    pub async fn boot(&mut self) -> io::Result<()> {
        fs::create_dir_all(&self.dir).await?;
        let mut max_index = None;
        let mut read = fs::read_dir(&self.dir).await?;
        while let Some(entry) = read.next_entry().await? {
            if let Some(index) = parse_segment_index(&entry.file_name()) {
                max_index = Some(max_index.map_or(index, |m: u64| m.max(index)));
            }
        }
        self.next_index = max_index.map_or(0, |m| m + 1);
        Ok(())
    }

    /// Returns whether a rotation happened — the caller uses it to decide
    /// when pruning is worth a directory scan ([`Self::maybe_prune`]).
    pub async fn append(&mut self, entries: &[QueryLogEntry]) -> io::Result<bool> {
        if entries.is_empty() {
            return Ok(false);
        }
        let mut buf = String::new();
        for entry in entries {
            let line = serde_json::to_string(entry).map_err(io::Error::other)?;
            buf.push_str(&line);
            buf.push('\n');
        }
        let bytes = buf.into_bytes();

        let mut rotated = false;
        if self.current_path.is_none()
            || (self.current_size > 0 && self.current_size + bytes.len() as u64 > MAX_SEGMENT_BYTES)
        {
            self.rotate();
            rotated = true;
        }
        let path = self
            .current_path
            .clone()
            .expect("rotate always sets current_path");
        fs::create_dir_all(&self.dir).await?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        file.write_all(&bytes).await?;
        self.current_size += bytes.len() as u64;
        Ok(rotated)
    }

    fn rotate(&mut self) {
        self.current_path = Some(self.dir.join(segment_name(self.next_index)));
        self.next_index += 1;
        self.current_size = 0;
    }

    pub fn current_path(&self) -> Option<&Path> {
        self.current_path.as_deref()
    }

    /// Prunes when it can matter: after a rotation (the only time total size
    /// grows past a segment boundary), on the first call after boot (previous
    /// runs' segments may have aged out), and at least every
    /// [`PRUNE_INTERVAL`] (the age cap must advance even with no rotation).
    /// Otherwise skips the directory scan entirely.
    pub async fn maybe_prune(
        &mut self,
        retention_days: u32,
        retention_max_mb: u32,
        now: SystemTime,
        rotated: bool,
    ) -> io::Result<()> {
        let due = rotated
            || self
                .last_prune
                .is_none_or(|at| at.elapsed() >= PRUNE_INTERVAL);
        if !due {
            return Ok(());
        }
        self.last_prune = Some(Instant::now());
        self.prune(retention_days, retention_max_mb, now).await
    }

    /// Deletes the oldest segments (zero-padded index names sort
    /// chronologically) until every remaining one is within `retention_days`
    /// and the total is within `retention_max_mb`. Never deletes the segment
    /// currently open for appending.
    pub async fn prune(
        &self,
        retention_days: u32,
        retention_max_mb: u32,
        now: SystemTime,
    ) -> io::Result<()> {
        let mut files = Vec::new();
        let mut read = match fs::read_dir(&self.dir).await {
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        while let Some(entry) = read.next_entry().await? {
            if parse_segment_index(&entry.file_name()).is_none() {
                continue;
            }
            let metadata = entry.metadata().await?;
            files.push((entry.path(), metadata.len(), metadata.modified()?));
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let max_age = Duration::from_secs(u64::from(retention_days) * 86_400);
        let cap_bytes = u64::from(retention_max_mb) * 1024 * 1024;
        let mut total: u64 = files.iter().map(|(_, size, _)| *size).sum();

        for (path, size, modified) in files {
            if Some(path.as_path()) == self.current_path() {
                break;
            }
            let too_old = now.duration_since(modified).unwrap_or_default() > max_age;
            let over_cap = total > cap_bytes;
            if !too_old && !over_cap {
                break;
            }
            fs::remove_file(&path).await?;
            total = total.saturating_sub(size);
        }
        Ok(())
    }
}

fn segment_name(index: u64) -> String {
    format!("seg-{index:010}.jsonl")
}

fn parse_segment_index(file_name: &std::ffi::OsStr) -> Option<u64> {
    file_name
        .to_str()?
        .strip_prefix("seg-")?
        .strip_suffix(".jsonl")?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use fah_model::{Query, QueryEvent, QueryType, Verdict};

    use super::*;

    fn entry(sequence: u64) -> QueryLogEntry {
        QueryLogEntry {
            sequence,
            event: fah_model::Event::dns(QueryEvent::new(
                Query::new(
                    "example.com",
                    QueryType::A,
                    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                    SystemTime::now(),
                ),
                Verdict::Pass,
                std::time::Duration::from_micros(100),
                false,
                true,
                false,
            )),
            client_name: None,
        }
    }

    #[tokio::test]
    async fn append_writes_jsonl_lines_readable_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = SegmentWriter::new(dir.path().to_path_buf());
        writer.boot().await.unwrap();
        writer.append(&[entry(0), entry(1)]).await.unwrap();

        let path = writer.current_path().unwrap().to_path_buf();
        let text = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(text.lines().count(), 2);
        let parsed: QueryLogEntry = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(parsed.sequence, 0);
    }

    #[tokio::test]
    async fn boot_resumes_numbering_from_existing_segments() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("seg-0000000003.jsonl"), "{}\n")
            .await
            .unwrap();

        let mut writer = SegmentWriter::new(dir.path().to_path_buf());
        writer.boot().await.unwrap();
        writer.append(&[entry(0)]).await.unwrap();

        assert_eq!(
            writer
                .current_path()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            "seg-0000000004.jsonl"
        );
    }

    #[tokio::test]
    async fn oversized_batch_rotates_to_a_new_segment() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = SegmentWriter::new(dir.path().to_path_buf());
        writer.boot().await.unwrap();

        let big_batch: Vec<QueryLogEntry> = (0..20_000).map(entry).collect();
        writer.append(&big_batch).await.unwrap();
        let first_path = writer.current_path().unwrap().to_path_buf();

        writer.append(&[entry(999_999)]).await.unwrap();
        let second_path = writer.current_path().unwrap().to_path_buf();

        assert_ne!(first_path, second_path);
    }

    #[tokio::test]
    async fn prune_deletes_segments_older_than_retention_days() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("seg-0000000000.jsonl");
        tokio::fs::write(&old_path, "{}\n").await.unwrap();

        let old_time = SystemTime::now() - Duration::from_secs(30 * 86_400);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&old_path)
            .unwrap()
            .set_modified(old_time)
            .unwrap();

        let mut writer = SegmentWriter::new(dir.path().to_path_buf());
        writer.boot().await.unwrap();
        // A fresh, empty "current" segment so the old one isn't the active file.
        writer.append(&[entry(0)]).await.unwrap();

        writer.prune(7, 500, SystemTime::now()).await.unwrap();

        assert!(
            !old_path.exists(),
            "segment older than retention_days must be pruned"
        );
    }

    #[tokio::test]
    async fn prune_deletes_oldest_segments_over_the_size_cap() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(
            dir.path().join("seg-0000000000.jsonl"),
            vec![b'x'; 2 * 1024 * 1024],
        )
        .await
        .unwrap();
        tokio::fs::write(
            dir.path().join("seg-0000000001.jsonl"),
            vec![b'x'; 2 * 1024 * 1024],
        )
        .await
        .unwrap();

        let mut writer = SegmentWriter::new(dir.path().to_path_buf());
        writer.boot().await.unwrap();
        writer.append(&[entry(0)]).await.unwrap(); // seg-0000000002 becomes current

        writer.prune(365, 3, SystemTime::now()).await.unwrap();

        assert!(!dir.path().join("seg-0000000000.jsonl").exists());
    }

    #[tokio::test]
    async fn maybe_prune_skips_the_scan_until_rotation_or_interval() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("seg-0000000000.jsonl");
        tokio::fs::write(&old_path, "{}\n").await.unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&old_path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(30 * 86_400))
            .unwrap();

        let mut writer = SegmentWriter::new(dir.path().to_path_buf());
        writer.boot().await.unwrap();
        writer.append(&[entry(0)]).await.unwrap();

        // First call after boot always prunes (last_prune is None).
        writer
            .maybe_prune(7, 500, SystemTime::now(), false)
            .await
            .unwrap();
        assert!(!old_path.exists());

        // A fresh old file now: not rotated + within the interval = skipped.
        tokio::fs::write(&old_path, "{}\n").await.unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&old_path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(30 * 86_400))
            .unwrap();
        writer
            .maybe_prune(7, 500, SystemTime::now(), false)
            .await
            .unwrap();
        assert!(
            old_path.exists(),
            "no rotation, interval not elapsed: no scan"
        );

        // rotated = true forces the prune regardless of the interval.
        writer
            .maybe_prune(7, 500, SystemTime::now(), true)
            .await
            .unwrap();
        assert!(!old_path.exists());
    }

    #[tokio::test]
    async fn prune_never_deletes_the_active_segment() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = SegmentWriter::new(dir.path().to_path_buf());
        writer.boot().await.unwrap();
        writer.append(&[entry(0)]).await.unwrap();
        let active = writer.current_path().unwrap().to_path_buf();

        std::fs::OpenOptions::new()
            .write(true)
            .open(&active)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(1_000 * 86_400))
            .unwrap();

        writer.prune(0, 0, SystemTime::now()).await.unwrap();

        assert!(
            active.exists(),
            "the currently-open segment must survive pruning"
        );
    }
}

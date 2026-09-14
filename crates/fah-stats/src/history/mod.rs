//! Long-term observability history on `/data` (ADR-0002: flat JSONL, no
//! embedded DB). The 24h aggregate ring ([`crate::bucket`]) can't hold more
//! than a day; this module captures each completed hour before its slot rolls
//! over, plus a daily top-N ([`rollup`]) and a per-interval perf/system/cache
//! sample series ([`perf`]), so a future dashboard has 30/60/90 days to chart.
//!
//! Everything here runs on the flush/sample schedulers, never on the per-query
//! path (hard rule 3): [`Stats`](crate::Stats) extracts a cheap snapshot under
//! its locks (or the binary reads telemetry snapshots) and hands plain data to
//! the writers for off-thread I/O.
//!
//! [`reader`] is the other direction — the range-scoped, bounded reads behind
//! `GET /api/v1/history/*`. It shares no state with the writers (paths only),
//! so a read never blocks a flush.

pub(crate) mod date;
pub(crate) mod perf;
pub(crate) mod reader;
pub(crate) mod rollup;

pub(crate) use perf::PerfWriter;
pub(crate) use reader::HistoryReader;
pub(crate) use rollup::RollupWriter;

use std::io;
use std::path::Path;

use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Appends one line (plus `\n`) to `path`, creating the parent directory on
/// demand. Shared by the rollup and perf writers — both are append-only JSONL.
pub(crate) async fn append_line(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    let mut framed = String::with_capacity(line.len() + 1);
    framed.push_str(line);
    framed.push('\n');
    file.write_all(framed.as_bytes()).await?;
    file.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_line_is_written_with_one_terminator_into_a_directory_made_on_demand() {
        let data = tempfile::tempdir().expect("data volume");
        let path = data.path().join("history").join("rollup.jsonl");

        append_line(&path, r#"{"hour":1}"#).await.expect("append");

        let written = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(written, "{\"hour\":1}\n");
    }

    #[tokio::test]
    async fn every_appended_line_carries_its_own_terminator() {
        let data = tempfile::tempdir().expect("data volume");
        let path = data.path().join("perf.jsonl");

        for line in ["one", "two", "three"] {
            append_line(&path, line).await.expect("append");
        }

        let written = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(written, "one\ntwo\nthree\n");
        assert_eq!(
            written.matches('\n').count(),
            3,
            "a record written without its terminator glues itself to the next one, and the \
             reader skips the line that results"
        );
    }
}

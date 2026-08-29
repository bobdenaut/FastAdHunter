//! `/data` raw-copy cache for rule lists (RULE_ENGINE.md §List lifecycle):
//! boot compiles from these cached copies without touching the network.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::fs;

fn cache_path(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join("lists").join(format!("{id}.raw"))
}

fn validators_path(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join("lists").join(format!("{id}.validators"))
}

pub(super) async fn read_validators(data_dir: &Path, id: &str) -> super::source::Validators {
    let Ok(text) = fs::read_to_string(validators_path(data_dir, id)).await else {
        return super::source::Validators::default();
    };
    let mut lines = text.lines();
    let field = |line: Option<&str>| line.map(str::to_owned).filter(|value| !value.is_empty());
    super::source::Validators {
        etag: field(lines.next()),
        last_modified: field(lines.next()),
    }
}

pub(super) async fn write_validators(
    data_dir: &Path,
    id: &str,
    validators: &super::source::Validators,
) {
    let path = validators_path(data_dir, id);
    if validators.is_empty() {
        let _ = fs::remove_file(&path).await;
        return;
    }
    let text = format!(
        "{}\n{}\n",
        validators.etag.as_deref().unwrap_or(""),
        validators.last_modified.as_deref().unwrap_or("")
    );
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent).await;
    }
    if let Err(err) = fs::write(&path, text).await {
        tracing::debug!(list = id, error = %err, "failed to persist list validators");
    }
}

/// Reads a list's cached raw text, if a cache file exists yet. First-ever
/// boot before any successful fetch has none — the caller treats that list as
/// absent from the initial ruleset until the async refresh lands
/// (RULE_ENGINE.md: boot must not wait on the network).
pub(super) async fn read(data_dir: &Path, id: &str) -> Option<String> {
    fs::read_to_string(cache_path(data_dir, id)).await.ok()
}

/// Writes a list's raw text to `/data`, replacing any previous cache. Atomic:
/// write to a sibling temp file then rename, so a crash mid-write never
/// leaves a half-written cache for the next boot to read (mirrors
/// `fah-config`'s config-file write, independently for `/data`).
pub(super) async fn write(data_dir: &Path, id: &str, text: &str) -> std::io::Result<()> {
    let path = cache_path(data_dir, id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".tmp.{}", std::process::id()));
    let tmp = path.with_file_name(tmp_name);

    fs::write(&tmp, text).await?;
    fs::rename(&tmp, &path).await
}

/// How long ago this list's cached copy was written, relative to `now`.
///
/// [`write`] renames the file into place on every successful fetch, so its
/// mtime *is* the wall-clock time of that list's last successful refresh — the
/// durable form of a clock the process cannot keep for itself, because
/// `ListManager::last_attempted` holds `tokio::time::Instant`s that reset with
/// the process. `now` is passed in rather than read here so one seeding pass
/// measures every list against the same instant.
///
/// `None` means "no usable age": no cache file, an mtime the platform will not
/// report, or an mtime in the future. That last case is real on the RB5009 —
/// it has no battery-backed RTC, so a container starting before NTP syncs can
/// see a clock behind its own files. Every `None` makes the caller leave the
/// list due, which is exactly what it did before this existed.
pub(super) async fn age(data_dir: &Path, id: &str, now: SystemTime) -> Option<Duration> {
    let modified = fs::metadata(cache_path(data_dir, id))
        .await
        .ok()?
        .modified()
        .ok()?;
    now.duration_since(modified).ok()
}

/// Deletes a list's cached raw text (`DELETE /api/v1/lists/{id}`). A list
/// that never fetched successfully has no cache file — that is not an error,
/// so a missing file reports success.
pub(super) async fn remove(data_dir: &Path, id: &str) -> std::io::Result<()> {
    let _ = fs::remove_file(validators_path(data_dir, id)).await;
    match fs::remove_file(cache_path(data_dir, id)).await {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Deletes cached copies no list claims any more, returning `(id, bytes)` for
/// each one removed. Boot only — see `ListManager::remove_orphaned_copies` for
/// why.
///
/// `keep` must list **every** id entitled to a cache file, which is not the
/// same as every id that compiles: a *disabled* list keeps its copy so
/// re-enabling is instant, and `user-rules` has a copy but never appears in
/// `[[rules.lists]]` at all. Passing a keep-set built from the enabled lists
/// would delete both.
///
/// Only `{id}.raw` is considered. The `.raw.tmp.{pid}` sidecars [`write`]
/// renames from are deliberately left alone: they cannot be told apart from a
/// write in flight, and a leaked one is a few MB that the next successful
/// refresh of that list replaces anyway.
pub(super) async fn remove_orphans(
    data_dir: &Path,
    keep: &HashSet<Arc<str>>,
) -> Vec<(String, u64)> {
    let dir = data_dir.join("lists");
    let mut listing = match fs::read_dir(&dir).await {
        Ok(listing) => listing,
        // No cache directory yet — a first-ever boot has nothing to sweep, and
        // any other error means the sweep simply does not run this time.
        Err(_) => return Vec::new(),
    };

    let mut removed = Vec::new();
    loop {
        let entry = match listing.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => {
                tracing::warn!(error = %err, "stopped scanning cached copies for orphans");
                break;
            }
        };

        let path = entry.path();
        let extension = path.extension().and_then(|ext| ext.to_str());
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if keep.contains(id) {
            continue;
        }
        if extension == Some("validators") {
            let _ = fs::remove_file(&path).await;
            continue;
        }
        if extension != Some("raw") {
            continue;
        }

        let bytes = entry.metadata().await.map(|meta| meta.len()).unwrap_or(0);
        let id = id.to_string();
        match fs::remove_file(&path).await {
            Ok(()) => removed.push((id, bytes)),
            Err(err) => {
                tracing::warn!(list = %id, error = %err, "failed to delete orphaned cached copy");
            }
        }
    }
    removed
}

/// Test-only: forces a cache file's mtime, so a test can present a cached copy
/// as arbitrarily old (or, for the clock-skew case, as newer than "now")
/// without waiting. `File::set_times` needs the file opened for writing, not
/// just its directory.
#[cfg(test)]
pub(super) fn set_modified(data_dir: &Path, id: &str, when: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(cache_path(data_dir, id))
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(when))
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_cache_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path(), "no-such-list").await.is_none());
    }

    #[tokio::test]
    async fn write_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "oisd-basic", "||ads.example.com^\n")
            .await
            .unwrap();
        assert_eq!(
            read(dir.path(), "oisd-basic").await.as_deref(),
            Some("||ads.example.com^\n")
        );
    }

    #[tokio::test]
    async fn write_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested").join("deeper");
        write(&nested, "user-rules", "example.org\n").await.unwrap();
        assert_eq!(
            read(&nested, "user-rules").await.as_deref(),
            Some("example.org\n")
        );
    }

    #[tokio::test]
    async fn a_missing_cache_file_has_no_age() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            age(dir.path(), "no-such-list", SystemTime::now()).await,
            None
        );
    }

    #[tokio::test]
    async fn age_measures_back_to_the_mtime() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "oisd-basic", "x\n").await.unwrap();
        let six_hours = Duration::from_secs(6 * 3600);
        set_modified(dir.path(), "oisd-basic", SystemTime::now() - six_hours);

        let age = age(dir.path(), "oisd-basic", SystemTime::now())
            .await
            .expect("a written cache file has an age");
        // Loose bound: the two `now`s are read a few microseconds apart, and
        // some filesystems round mtimes to whole seconds.
        assert!(
            age.abs_diff(six_hours) < Duration::from_secs(5),
            "expected ~6h, got {age:?}"
        );
    }

    /// The RB5009 has no battery-backed RTC, so a container can start with the
    /// clock behind files written before the reboot. A negative age must read
    /// as "unknown", never wrap into a huge positive one.
    #[tokio::test]
    async fn an_mtime_in_the_future_has_no_age() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "oisd-basic", "x\n").await.unwrap();
        set_modified(
            dir.path(),
            "oisd-basic",
            SystemTime::now() + Duration::from_secs(3600),
        );

        assert_eq!(age(dir.path(), "oisd-basic", SystemTime::now()).await, None);
    }

    #[tokio::test]
    async fn overwrite_replaces_previous_content() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "oisd-basic", "old\n").await.unwrap();
        write(dir.path(), "oisd-basic", "new\n").await.unwrap();
        assert_eq!(
            read(dir.path(), "oisd-basic").await.as_deref(),
            Some("new\n")
        );
    }
}

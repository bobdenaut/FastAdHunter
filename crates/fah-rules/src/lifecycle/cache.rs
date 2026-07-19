//! `/data` raw-copy cache for rule lists (RULE_ENGINE.md §List lifecycle):
//! boot compiles from these cached copies without touching the network.

use std::path::{Path, PathBuf};

use tokio::fs;

fn cache_path(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join("lists").join(format!("{id}.raw"))
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

/// Deletes a list's cached raw text (`DELETE /api/v1/lists/{id}`). A list
/// that never fetched successfully has no cache file — that is not an error,
/// so a missing file reports success.
pub(super) async fn remove(data_dir: &Path, id: &str) -> std::io::Result<()> {
    match fs::remove_file(cache_path(data_dir, id)).await {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
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

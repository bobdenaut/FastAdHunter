//! Periodic snapshot of aggregates + client registry to `/data`
//! (ARCHITECTURE.md §Data & Persistence), loaded on boot so a restart isn't
//! zero'd — atomic write, mirroring `fah-config`'s config-file write and
//! `fah-rules`'s list cache.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::aggregates::Aggregates;
use crate::client_registry::ClientRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SnapshotData {
    pub aggregates: Aggregates,
    pub clients: ClientRegistry,
}

fn snapshot_path(data_dir: &Path) -> PathBuf {
    data_dir.join("stats").join("snapshot.json")
}

/// `None` covers both "never snapshotted yet" and a corrupt/unreadable file —
/// either way boot proceeds with empty state rather than failing to start.
pub(crate) async fn load(data_dir: &Path) -> Option<SnapshotData> {
    let text = tokio::fs::read_to_string(snapshot_path(data_dir))
        .await
        .ok()?;
    serde_json::from_str(&text).ok()
}

pub(crate) async fn save(data_dir: &Path, data: &SnapshotData) -> std::io::Result<()> {
    let path = snapshot_path(data_dir);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let text = serde_json::to_string(data).map_err(std::io::Error::other)?;

    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".tmp.{}", std::process::id()));
    let tmp = path.with_file_name(tmp_name);

    tokio::fs::write(&tmp, text).await?;
    tokio::fs::rename(&tmp, &path).await
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use fah_model::Verdict;

    use super::*;

    #[tokio::test]
    async fn missing_snapshot_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).await.is_none());
    }

    #[tokio::test]
    async fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let mut aggregates = Aggregates::default();
        aggregates.record("ads.example.com", &Verdict::Pass, true, now);
        let data = SnapshotData {
            aggregates,
            clients: ClientRegistry::default(),
        };

        save(dir.path(), &data).await.unwrap();
        let loaded = load(dir.path()).await.unwrap();
        assert_eq!(loaded.aggregates.queries_total(now), 1);
    }

    #[tokio::test]
    async fn corrupt_snapshot_file_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = snapshot_path(dir.path());
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&path, "not json").await.unwrap();
        assert!(load(dir.path()).await.is_none());
    }
}

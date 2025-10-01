use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::fs as afs;
use tokio::io::{AsyncReadExt, ErrorKind};
use uuid::Uuid;

const SNAPSHOT_ROOT: &str = ".snapshots";
const METADATA_FILE: &str = "metadata.json";
const FILES_DIR: &str = "files";
const TMP_SUFFIX: &str = ".tmp";

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("project '{0}' not found")]
    ProjectMissing(String),
    #[error("snapshot '{0}' not found")]
    SnapshotMissing(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl SnapshotError {
    pub fn not_found(project: &str) -> Self {
        SnapshotError::ProjectMissing(project.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ProjectSnapshotMetadata {
    pub id: String,
    pub project: String,
    pub created: String,
    pub created_ms: u64,
    pub bytes: u64,
    pub files: u64,
    pub digest: String,
    pub path: String,
    #[serde(default)]
    pub skipped: u64,
}

#[derive(Default)]
struct CopyStats {
    bytes: u64,
    files: u64,
    skipped: u64,
    digest: Sha256,
}

pub async fn create_snapshot(
    project_root: &Path,
    project: &str,
) -> Result<ProjectSnapshotMetadata, SnapshotError> {
    let meta = afs::metadata(project_root)
        .await
        .map_err(|err| match err.kind() {
            ErrorKind::NotFound => SnapshotError::not_found(project),
            _ => SnapshotError::Io(err),
        })?;
    if !meta.is_dir() {
        return Err(SnapshotError::ProjectMissing(project.to_string()));
    }

    let base = snapshots_root(project_root, project)?;
    afs::create_dir_all(&base).await?;

    let snapshot_id = Uuid::new_v4().to_string();
    let created = Utc::now();
    let temp_dir = base.join(format!("{}{}", snapshot_id, TMP_SUFFIX));
    if afs::metadata(&temp_dir).await.is_ok() {
        let _ = afs::remove_dir_all(&temp_dir).await;
    }
    afs::create_dir_all(&temp_dir).await?;
    let files_dir = temp_dir.join(FILES_DIR);
    afs::create_dir(&files_dir).await?;

    let mut stats = CopyStats::default();
    copy_project_tree(project_root, &files_dir, project_root, &mut stats).await?;

    let digest_hex = format!("{:x}", stats.digest.finalize());
    let metadata = ProjectSnapshotMetadata {
        id: snapshot_id.clone(),
        project: project.to_string(),
        created: created.to_rfc3339_opts(SecondsFormat::Millis, true),
        created_ms: created.timestamp_millis() as u64,
        bytes: stats.bytes,
        files: stats.files,
        digest: digest_hex,
        path: snapshot_rel_path(project, &snapshot_id),
        skipped: stats.skipped,
    };

    let metadata_bytes = serde_json::to_vec_pretty(&metadata)?;
    afs::write(temp_dir.join(METADATA_FILE), metadata_bytes).await?;

    let final_dir = base.join(&snapshot_id);
    match afs::rename(&temp_dir, &final_dir).await {
        Ok(_) => Ok(metadata),
        Err(err) => {
            let _ = afs::remove_dir_all(&temp_dir).await;
            Err(SnapshotError::Io(err))
        }
    }
}

pub async fn list_snapshots(
    project_root: &Path,
    project: &str,
    limit: usize,
) -> Result<Vec<ProjectSnapshotMetadata>, SnapshotError> {
    let base = snapshots_root(project_root, project)?;
    let mut entries = match afs::read_dir(&base).await {
        Ok(rd) => rd,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(SnapshotError::Io(err)),
    };
    let mut snapshots = Vec::new();
    while let Some(ent) = entries.next_entry().await? {
        let file_type = ent.file_type().await?;
        if !file_type.is_dir() {
            continue;
        }
        let id_os = ent.file_name();
        let Some(id_str) = id_os.to_str() else {
            continue;
        };
        match load_snapshot_metadata(&ent.path(), project, id_str).await {
            Ok(meta) => snapshots.push(meta),
            Err(_) => continue,
        }
    }
    snapshots.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
    if snapshots.len() > limit {
        snapshots.truncate(limit);
    }
    Ok(snapshots)
}

fn snapshots_root(project_root: &Path, project: &str) -> Result<PathBuf, SnapshotError> {
    let parent = project_root
        .parent()
        .ok_or_else(|| SnapshotError::ProjectMissing(project.to_string()))?;
    Ok(parent.join(SNAPSHOT_ROOT).join(project))
}

fn snapshot_rel_path(project: &str, id: &str) -> String {
    format!("{}/{}/{}", SNAPSHOT_ROOT, project, id)
}

pub async fn restore_snapshot(
    project_root: &Path,
    project: &str,
    snapshot_id: &str,
) -> Result<ProjectSnapshotMetadata, SnapshotError> {
    let base = snapshots_root(project_root, project)?;
    let snapshot_dir = base.join(snapshot_id);
    let files_dir = snapshot_dir.join(FILES_DIR);
    let metadata = load_snapshot_metadata(&snapshot_dir, project, snapshot_id).await?;
    if !afs::metadata(&files_dir)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false)
    {
        return Err(SnapshotError::SnapshotMissing(snapshot_id.to_string()));
    }

    let temp_restore = project_root.with_extension(format!("restore-{}", Uuid::new_v4()));
    if afs::metadata(&temp_restore).await.is_ok() {
        let _ = afs::remove_dir_all(&temp_restore).await;
    }
    replicate_directory(&files_dir, &temp_restore).await?;

    let backup_dir = project_root.with_extension(format!("backup-{}", Uuid::new_v4()));
    if afs::metadata(project_root).await.is_ok() {
        if let Err(err) = afs::rename(project_root, &backup_dir).await {
            let _ = afs::remove_dir_all(&temp_restore).await;
            return Err(SnapshotError::Io(err));
        }
    }

    match afs::rename(&temp_restore, project_root).await {
        Ok(_) => {
            let _ = afs::remove_dir_all(&backup_dir).await;
            Ok(metadata)
        }
        Err(err) => {
            let _ = afs::remove_dir_all(&temp_restore).await;
            if afs::metadata(&backup_dir).await.is_ok() {
                let _ = afs::rename(&backup_dir, project_root).await;
            }
            Err(SnapshotError::Io(err))
        }
    }
}

async fn load_snapshot_metadata(
    snapshot_dir: &Path,
    project: &str,
    snapshot_id: &str,
) -> Result<ProjectSnapshotMetadata, SnapshotError> {
    let metadata_path = snapshot_dir.join(METADATA_FILE);
    let bytes = afs::read(&metadata_path)
        .await
        .map_err(|err| match err.kind() {
            ErrorKind::NotFound => SnapshotError::SnapshotMissing(snapshot_id.to_string()),
            _ => SnapshotError::Io(err),
        })?;
    let mut meta: ProjectSnapshotMetadata = serde_json::from_slice(&bytes)?;
    if meta.project.is_empty() {
        meta.project = project.to_string();
    }
    if meta.path.is_empty() {
        meta.path = snapshot_rel_path(project, snapshot_id);
    }
    Ok(meta)
}

async fn copy_project_tree(
    source_root: &Path,
    dest_root: &Path,
    base_root: &Path,
    stats: &mut CopyStats,
) -> Result<(), std::io::Error> {
    let mut stack = vec![(source_root.to_path_buf(), dest_root.to_path_buf())];
    while let Some((src_dir, dst_dir)) = stack.pop() {
        afs::create_dir_all(&dst_dir).await?;
        let mut dir = match afs::read_dir(&src_dir).await {
            Ok(rd) => rd,
            Err(err) => {
                if err.kind() == ErrorKind::NotFound {
                    continue;
                }
                return Err(err);
            }
        };
        let mut entries: Vec<(String, PathBuf, fs::Metadata)> = Vec::new();
        while let Some(ent) = dir.next_entry().await? {
            let meta = match ent.metadata().await {
                Ok(m) => m,
                Err(err) => {
                    if err.kind() == ErrorKind::NotFound {
                        continue;
                    }
                    return Err(err);
                }
            };
            let name = ent.file_name().to_string_lossy().to_string();
            entries.push((name, ent.path(), meta));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name_str, entry_path, meta) in entries {
            if name_str == SNAPSHOT_ROOT {
                continue;
            }
            let dest_path = dst_dir.join(&name_str);
            if meta.is_dir() {
                update_digest_for_dir(stats, base_root, &entry_path, &name_str);
                stack.push((entry_path, dest_path));
            } else if meta.is_file() {
                let bytes = afs::copy(&entry_path, &dest_path).await?;
                record_file(stats, base_root, &entry_path, bytes).await?;
            } else {
                stats.skipped = stats.skipped.saturating_add(1);
            }
        }
    }
    Ok(())
}

async fn replicate_directory(source_root: &Path, dest_root: &Path) -> Result<(), std::io::Error> {
    let mut stack = vec![(source_root.to_path_buf(), dest_root.to_path_buf())];
    while let Some((src_dir, dst_dir)) = stack.pop() {
        afs::create_dir_all(&dst_dir).await?;
        let mut dir = match afs::read_dir(&src_dir).await {
            Ok(rd) => rd,
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        while let Some(ent) = dir.next_entry().await? {
            let name = ent.file_name();
            let dest_path = dst_dir.join(&name);
            let entry_path = ent.path();
            let file_type = ent.file_type().await?;
            if file_type.is_dir() {
                stack.push((entry_path, dest_path));
            } else if file_type.is_file() {
                afs::copy(&entry_path, &dest_path).await?;
            }
        }
    }
    Ok(())
}

fn normalise_rel(path: &Path) -> String {
    let mut value = path.to_string_lossy().into_owned();
    if std::path::MAIN_SEPARATOR != '/' {
        value = value.replace(std::path::MAIN_SEPARATOR, "/");
    }
    value
}

fn update_digest_for_dir(
    stats: &mut CopyStats,
    base_root: &Path,
    entry_path: &Path,
    name_str: &str,
) {
    if let Ok(rel) = entry_path.strip_prefix(base_root) {
        let rel_norm = normalise_rel(rel);
        let len_bytes = (rel_norm.len() as u64).to_le_bytes();
        stats.digest.update(len_bytes);
        stats.digest.update(rel_norm.as_bytes());
        stats.digest.update(b"D");
    } else {
        let len_bytes = (name_str.len() as u64).to_le_bytes();
        stats.digest.update(len_bytes);
        stats.digest.update(name_str.as_bytes());
        stats.digest.update(b"D");
    }
}

async fn record_file(
    stats: &mut CopyStats,
    base_root: &Path,
    entry_path: &Path,
    bytes_copied: u64,
) -> Result<(), std::io::Error> {
    stats.bytes = stats.bytes.saturating_add(bytes_copied);
    stats.files = stats.files.saturating_add(1);
    let (rel_norm, name_len_bytes) = match entry_path.strip_prefix(base_root) {
        Ok(rel) => {
            let norm = normalise_rel(rel);
            let len_bytes = (norm.len() as u64).to_le_bytes();
            (norm, len_bytes)
        }
        Err(_) => {
            let name = entry_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (name.clone(), (name.len() as u64).to_le_bytes())
        }
    };
    let mut file_hasher = Sha256::new();
    let mut file = afs::File::open(entry_path).await?;
    let mut buf = vec![0u8; 8192];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        file_hasher.update(&buf[..n]);
    }
    let file_digest = file_hasher.finalize();
    stats.digest.update(name_len_bytes);
    stats.digest.update(rel_norm.as_bytes());
    stats.digest.update(b"F");
    let size_bytes = bytes_copied.to_le_bytes();
    stats.digest.update(size_bytes);
    stats.digest.update(file_digest.as_slice());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn snapshot_roundtrip() {
        let dir = tempdir().unwrap();
        let projects_root = dir.path().join("projects");
        let project_dir = projects_root.join("demo");
        afs::create_dir_all(&project_dir).await.unwrap();
        afs::write(project_dir.join("README.md"), b"hello")
            .await
            .unwrap();

        let meta = create_snapshot(&project_dir, "demo").await.unwrap();
        assert_eq!(meta.project, "demo");
        assert_eq!(meta.files, 1);
        assert_eq!(meta.bytes, 5);
        assert!(meta.path.starts_with(".snapshots/"));

        let listed = list_snapshots(&project_dir, "demo", 5).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, meta.id);

        let latest = list_snapshots(&project_dir, "demo", 1).await.unwrap();
        assert_eq!(
            latest.first().map(|s| s.id.as_str()),
            Some(meta.id.as_str())
        );
    }

    #[tokio::test]
    async fn restore_snapshot_replaces_contents() {
        let dir = tempdir().unwrap();
        let projects_root = dir.path().join("projects");
        let project_dir = projects_root.join("demo");
        afs::create_dir_all(project_dir.join("src")).await.unwrap();
        afs::write(project_dir.join("README.md"), b"before")
            .await
            .unwrap();
        afs::write(project_dir.join("src/code.rs"), b"fn main() {}")
            .await
            .unwrap();

        let meta = create_snapshot(&project_dir, "demo").await.unwrap();

        afs::write(project_dir.join("README.md"), b"after")
            .await
            .unwrap();
        afs::write(project_dir.join("temp.txt"), b"temp")
            .await
            .unwrap();

        restore_snapshot(&project_dir, "demo", &meta.id)
            .await
            .expect("restore snapshot");

        let restored = afs::read(project_dir.join("README.md")).await.unwrap();
        assert_eq!(restored, b"before");
        assert!(afs::metadata(project_dir.join("temp.txt")).await.is_err());
        assert!(afs::metadata(project_dir.join("src/code.rs")).await.is_ok());
    }

    #[tokio::test]
    async fn snapshot_digest_changes_when_file_content_changes() {
        let dir = tempdir().unwrap();
        let projects_root = dir.path().join("projects");
        let project_dir = projects_root.join("demo");
        afs::create_dir_all(&project_dir).await.unwrap();
        afs::write(project_dir.join("data.txt"), b"abc")
            .await
            .unwrap();

        let first = create_snapshot(&project_dir, "demo").await.unwrap();
        afs::write(project_dir.join("data.txt"), b"xyz")
            .await
            .unwrap();
        let second = create_snapshot(&project_dir, "demo").await.unwrap();

        assert_ne!(
            first.digest, second.digest,
            "snapshot digest should include file content changes"
        );
    }
}

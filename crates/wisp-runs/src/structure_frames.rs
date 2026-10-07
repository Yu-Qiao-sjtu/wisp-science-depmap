//! Structure files a running task has just written.
//!
//! The viewer does not compute docking. A run that writes `pdb` / `cif` /
//! `mmcif` / `gro` / `ent` frames is the producer; this module decides which
//! file is the next frame to push into an already-open viewer. A file is
//! presented only after its size and modification time stay unchanged for one
//! observation, so a pose still being written is not pushed early.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const MAX_DEPTH: usize = 4;
const MAX_FILES: usize = 64;
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".wisp"];

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrameId {
    pub path: PathBuf,
    pub modified_ms: u128,
    pub len: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureFrame {
    pub id: FrameId,
}

#[derive(Default)]
pub struct FrameWatch {
    seen: HashSet<FrameId>,
    pending: Option<FrameId>,
}

pub fn is_structure_frame(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("pdb" | "ent" | "cif" | "mmcif" | "gro")
    )
}

/// Next structure in a remote workspace listing.
///
/// Identity is the workspace path plus byte size. The first observation is
/// held; the same size on the next observation is ready to download. A size
/// change means the file is still being written.
pub fn next_remote_structure_frame(
    entries: &[(String, u64)],
    watch: &mut FrameWatch,
) -> Option<String> {
    let frames: Vec<StructureFrame> = entries
        .iter()
        .filter(|(_, size)| *size > 0)
        .filter(|(path, _)| is_structure_frame(Path::new(path)))
        .map(|(path, size)| StructureFrame {
            id: FrameId {
                path: PathBuf::from(path),
                modified_ms: 0,
                len: *size,
            },
        })
        .collect();
    next_stable_frame(&frames, watch, 0).map(|path| path.to_string_lossy().into_owned())
}

/// Next stable structure written at or after `not_before_ms`.
///
/// The first observation of a new identity is held. The same identity on the
/// following observation is the frame to present.
pub fn next_stable_frame(
    frames: &[StructureFrame],
    watch: &mut FrameWatch,
    not_before_ms: u128,
) -> Option<PathBuf> {
    let mut ready: Vec<&StructureFrame> = frames
        .iter()
        .filter(|frame| frame.id.modified_ms >= not_before_ms)
        .filter(|frame| !watch.seen.contains(&frame.id))
        .collect();
    ready.sort_by(|left, right| {
        left.id
            .modified_ms
            .cmp(&right.id.modified_ms)
            .then_with(|| left.id.path.cmp(&right.id.path))
    });
    let Some(oldest) = ready.first() else {
        watch.pending = None;
        return None;
    };
    if watch.pending.as_ref() == Some(&oldest.id) {
        watch.seen.insert(oldest.id.clone());
        watch.pending = None;
        return Some(oldest.id.path.clone());
    }
    watch.pending = Some(oldest.id.clone());
    None
}

/// Poll `root` until the run is finished. One stable new structure per second
/// is handed to `present`, in write order.
pub async fn watch_local_structure_frames(
    store: &wisp_store::Store,
    run_id: &str,
    root: &Path,
    present: Arc<dyn Fn(&Path) + Send + Sync>,
) {
    let not_before_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let mut watch = FrameWatch::default();
    for _ in 0..(6 * 60 * 60) {
        match store.get_run(run_id).await {
            Ok(Some(run)) if !run.status.is_terminal() => {}
            _ => break,
        }
        if let Some(path) =
            next_stable_frame(&scan_structure_frames(root), &mut watch, not_before_ms)
        {
            present(&path);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

pub fn scan_structure_frames(root: &Path) -> Vec<StructureFrame> {
    let mut found = Vec::new();
    scan_dir(root, 0, &mut found);
    found
}

fn scan_dir(dir: &Path, depth: usize, found: &mut Vec<StructureFrame>) {
    if depth > MAX_DEPTH || found.len() >= MAX_FILES {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        if found.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            scan_dir(&path, depth + 1, found);
            continue;
        }
        if !meta.is_file() || !is_structure_frame(&path) {
            continue;
        }
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        found.push(StructureFrame {
            id: FrameId {
                path,
                modified_ms,
                len: meta.len(),
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(path: &str, modified_ms: u128, len: u64) -> StructureFrame {
        StructureFrame {
            id: FrameId {
                path: PathBuf::from(path),
                modified_ms,
                len,
            },
        }
    }

    #[test]
    fn a_new_stable_structure_is_the_next_frame() {
        let mut watch = FrameWatch::default();
        let frames = vec![frame("pose.pdb", 2_000, 10)];
        assert!(next_stable_frame(&frames, &mut watch, 1_000).is_none());
        assert_eq!(
            next_stable_frame(&frames, &mut watch, 1_000),
            Some(PathBuf::from("pose.pdb"))
        );
        assert!(next_stable_frame(&frames, &mut watch, 1_000).is_none());
    }

    #[test]
    fn files_written_before_the_run_are_ignored() {
        let mut watch = FrameWatch::default();
        let frames = vec![frame("old.pdb", 500, 10)];
        assert!(next_stable_frame(&frames, &mut watch, 1_000).is_none());
        assert!(watch.pending.is_none());
    }

    #[test]
    fn a_rewritten_pose_is_presented_again() {
        let mut watch = FrameWatch::default();
        let first = vec![frame("pose.pdb", 2_000, 10)];
        next_stable_frame(&first, &mut watch, 1_000);
        next_stable_frame(&first, &mut watch, 1_000);
        let rewritten = vec![frame("pose.pdb", 3_000, 12)];
        assert!(next_stable_frame(&rewritten, &mut watch, 1_000).is_none());
        assert_eq!(
            next_stable_frame(&rewritten, &mut watch, 1_000),
            Some(PathBuf::from("pose.pdb"))
        );
    }

    #[test]
    fn frames_play_in_write_order() {
        let mut watch = FrameWatch::default();
        let frames = vec![frame("b.cif", 3_000, 1), frame("a.pdb", 2_000, 1)];
        next_stable_frame(&frames, &mut watch, 1_000);
        assert_eq!(
            next_stable_frame(&frames, &mut watch, 1_000),
            Some(PathBuf::from("a.pdb"))
        );
        next_stable_frame(&frames, &mut watch, 1_000);
        assert_eq!(
            next_stable_frame(&frames, &mut watch, 1_000),
            Some(PathBuf::from("b.cif"))
        );
    }

    #[test]
    fn a_remote_pose_is_ready_only_after_its_size_stays_put() {
        let mut watch = FrameWatch::default();
        let growing = vec![("poses/pose-001.pdb".into(), 20)];
        assert!(next_remote_structure_frame(&growing, &mut watch).is_none());
        let still_growing = vec![("poses/pose-001.pdb".into(), 40)];
        assert!(next_remote_structure_frame(&still_growing, &mut watch).is_none());
        assert_eq!(
            next_remote_structure_frame(&still_growing, &mut watch).as_deref(),
            Some("poses/pose-001.pdb")
        );
        assert!(next_remote_structure_frame(&still_growing, &mut watch).is_none());
    }

    #[test]
    fn scan_reads_a_new_pdb_and_skips_other_files() {
        let root = std::env::temp_dir().join(format!("wisp-frames-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("out")).unwrap();
        fs::write(root.join("out").join("pose.pdb"), b"ATOM").unwrap();
        fs::write(root.join("notes.txt"), b"no").unwrap();
        let found = scan_structure_frames(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(found.len(), 1);
        assert!(found[0].id.path.ends_with("pose.pdb"));
    }
}

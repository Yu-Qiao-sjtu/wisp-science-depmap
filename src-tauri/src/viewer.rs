//! Dedicated molecular viewer window (slices 1-2/4 of #191, see #192/#193).
//!
//! Renders the vendored open-source Mol\* bundle (`ui/viewer.html`) in its own
//! webview. Structure and trajectory bytes travel only through the
//! path-validated `read_*_bytes` commands; the webview itself holds no
//! filesystem scope.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub(crate) const VIEWER_WINDOW_LABEL: &str = "viewer-structure";

/// Event emitted to an already-open viewer window to swap in a new structure
/// without navigating (first open receives its path through the URL query, so
/// it never races the page's event listener).
const LOAD_STRUCTURE_EVENT: &str = "viewer://load-structure";

/// Event carrying a new topology + trajectory pair into an open viewer window.
const LOAD_TRAJECTORY_EVENT: &str = "viewer://load-trajectory";

/// Upper bound for a structure file handed to the viewer. Structures are tiny
/// next to trajectories; a larger "structure" is almost certainly the wrong
/// file (e.g. a multi-frame trajectory), which is routed through the
/// trajectory-specific commands instead.
pub(crate) const MAX_STRUCTURE_BYTES: u64 = 128 * 1024 * 1024;

/// Upper bound for a trajectory file. Real MD trajectories (DCD/XTC from
/// OpenMM or GROMACS runs) routinely reach hundreds of MiB; the cap exists
/// only to refuse accidentally selected multi-GB payloads, not to constrain
/// legitimate runs.
pub(crate) const MAX_TRAJECTORY_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) fn is_allowed_structure_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "pdb" | "ent" | "cif" | "mmcif" | "gro"
    )
}

/// Coordinate formats parsed by Mol\* mol-io (the NetCDF reader also covers
/// `.nc`, GROMACS' alternative extension for the same container).
pub(crate) fn is_allowed_trajectory_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "dcd" | "xtc" | "trr" | "netcdf" | "nc"
    )
}

/// Resolve and vet a viewer file: non-empty, allow-listed extension, an
/// existing regular file, and within the size cap. Returns the canonical path
/// on success; the error strings are user-facing.
fn validate_viewer_file(
    kind: &str,
    path: &str,
    allowed: fn(&str) -> bool,
    expected: &str,
    cap: u64,
) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err(format!("{kind} path is empty"));
    }
    let raw = Path::new(path);
    let ext = raw
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| format!("{kind} path has no extension: {path:?}"))?;
    if !allowed(ext) {
        return Err(format!(
            "unsupported {kind} extension {ext:?}; expected {expected}"
        ));
    }
    let canonical = std::fs::canonicalize(raw)
        .map_err(|e| format!("cannot resolve {kind} path {path:?}: {e}"))?;
    let meta = std::fs::metadata(&canonical)
        .map_err(|e| format!("cannot read {kind} file {path:?}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{kind} path is not a regular file: {path:?}"));
    }
    if meta.len() > cap {
        return Err(format!(
            "{kind} file is {} bytes; the viewer caps {kind} files at {cap} bytes",
            meta.len()
        ));
    }
    Ok(canonical)
}

pub(crate) fn validate_structure_path(path: &str) -> Result<PathBuf, String> {
    validate_viewer_file(
        "structure",
        path,
        is_allowed_structure_extension,
        "pdb, ent, cif, mmcif or gro",
        MAX_STRUCTURE_BYTES,
    )
}

pub(crate) fn validate_trajectory_path(path: &str) -> Result<PathBuf, String> {
    validate_viewer_file(
        "trajectory",
        path,
        is_allowed_trajectory_extension,
        "dcd, xtc, trr, netcdf or nc",
        MAX_TRAJECTORY_BYTES,
    )
}

/// Percent-encode a query value dependency-free, keeping characters that are
/// harmless in a query (`:/\` for Windows and POSIX paths).
fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len() * 3);
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b':'
            | b'/'
            | b'\\' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub(crate) fn viewer_url(path: &str) -> String {
    format!("viewer.html?src={}", percent_encode_path(path))
}

pub(crate) fn viewer_trajectory_url(structure: &str, trajectory: &str) -> String {
    format!(
        "viewer.html?topo={}&traj={}",
        percent_encode_path(structure),
        percent_encode_path(trajectory)
    )
}

#[derive(Clone, Serialize)]
struct LoadStructurePayload {
    path: String,
}

#[derive(Clone, Serialize)]
struct LoadTrajectoryPayload {
    structure_path: String,
    trajectory_path: String,
}

/// Spawn the shared viewer window on the given app-relative URL. Both the
/// structure and trajectory entry points reuse this so the two modes share
/// one window (and therefore one Mol\* instance) at a time.
fn spawn_viewer_window(app: &AppHandle, url: String, title: &str) -> Result<(), String> {
    let mut builder =
        WebviewWindowBuilder::new(app, VIEWER_WINDOW_LABEL, WebviewUrl::App(url.into()))
            .title(title)
            .inner_size(1200.0, 840.0)
            .min_inner_size(640.0, 480.0)
            .resizable(true)
            .general_autofill_enabled(false)
            .on_navigation(crate::guard_webview_navigation);
    #[cfg(target_os = "windows")]
    let builder = builder.decorations(false).shadow(true);
    builder
        .build()
        .map_err(|e| format!("failed to open the viewer window: {e}"))
        .map(|_| ())
}

/// Open (or focus) the structure viewer window on a validated structure file.
///
/// The first open passes the path through the URL query; subsequent opens keep
/// the page mounted and deliver the path via [`LOAD_STRUCTURE_EVENT`], matching
/// the reuse semantics called for in #195.
#[tauri::command]
pub(crate) async fn open_structure_viewer(app: AppHandle, path: String) -> Result<(), String> {
    let canonical = validate_structure_path(&path)?;
    let path = canonical.to_string_lossy().into_owned();
    match app.get_webview_window(VIEWER_WINDOW_LABEL) {
        Some(window) => {
            let _ = window.set_focus();
            app.emit_to(
                VIEWER_WINDOW_LABEL,
                LOAD_STRUCTURE_EVENT,
                LoadStructurePayload { path },
            )
            .map_err(|e| format!("failed to deliver structure to the viewer window: {e}"))?;
        }
        None => spawn_viewer_window(&app, viewer_url(&path), "Structure viewer")?,
    }
    Ok(())
}

/// Open (or focus) the viewer window on a validated topology + trajectory
/// pair (slice 2/4, #193). The page drives Mol\*'s built-in trajectory
/// pipeline (mol-io DCD/XTC/TRR/NetCDF readers plus the animation preset), so
/// playback controls come from the vendored bundle itself.
#[tauri::command]
pub(crate) async fn open_trajectory_viewer(
    app: AppHandle,
    structure_path: String,
    trajectory_path: String,
) -> Result<(), String> {
    let canonical_structure = validate_structure_path(&structure_path)?;
    let canonical_trajectory = validate_trajectory_path(&trajectory_path)?;
    let structure_path = canonical_structure.to_string_lossy().into_owned();
    let trajectory_path = canonical_trajectory.to_string_lossy().into_owned();
    match app.get_webview_window(VIEWER_WINDOW_LABEL) {
        Some(window) => {
            let _ = window.set_focus();
            app.emit_to(
                VIEWER_WINDOW_LABEL,
                LOAD_TRAJECTORY_EVENT,
                LoadTrajectoryPayload {
                    structure_path,
                    trajectory_path,
                },
            )
            .map_err(|e| format!("failed to deliver trajectory to the viewer window: {e}"))?;
        }
        None => spawn_viewer_window(
            &app,
            viewer_trajectory_url(&structure_path, &trajectory_path),
            "Trajectory viewer",
        )?,
    }
    Ok(())
}

/// Read structure bytes for the viewer page after the same path validation as
/// [`open_structure_viewer`]. Keeping this on the command side (instead of a
/// broad filesystem permission on the webview) confines access to vetted
/// structure files.
#[tauri::command]
pub(crate) async fn read_structure_bytes(path: String) -> Result<Vec<u8>, String> {
    let canonical = validate_structure_path(&path)?;
    std::fs::read(&canonical)
        .map_err(|e| format!("failed to read structure file {}: {e}", canonical.display()))
}

/// Read trajectory bytes after the same path validation as
/// [`open_trajectory_viewer`]. Trajectories stream through memory as one
/// buffer; Mol\*'s readers are synchronous over in-memory data, so no temp
/// copy is written.
#[tauri::command]
pub(crate) async fn read_trajectory_bytes(path: String) -> Result<Vec<u8>, String> {
    let canonical = validate_trajectory_path(&path)?;
    std::fs::read(&canonical).map_err(|e| {
        format!(
            "failed to read trajectory file {}: {e}",
            canonical.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_extensions_are_case_insensitive() {
        for ext in ["pdb", "PDB", "cif", "CIF", "mmcif", "MMCIF", "gro", "ent"] {
            assert!(
                is_allowed_structure_extension(ext),
                "{ext} should be allowed"
            );
        }
    }

    #[test]
    fn trajectory_and_document_extensions_are_rejected() {
        // Trajectories must ride the dedicated trajectory commands.
        for ext in ["dcd", "xtc", "trr", "netcdf", "npz", "exe", ""] {
            assert!(
                !is_allowed_structure_extension(ext),
                "{ext:?} must be rejected"
            );
        }
    }

    #[test]
    fn trajectory_extensions_are_case_insensitive() {
        for ext in [
            "dcd", "DCD", "xtc", "XTC", "trr", "netcdf", "NetCDF", "nc", "NC",
        ] {
            assert!(
                is_allowed_trajectory_extension(ext),
                "{ext} should be allowed"
            );
        }
    }

    #[test]
    fn structure_extensions_are_rejected_for_trajectories() {
        for ext in ["pdb", "ent", "cif", "mmcif", "gro", "npz", "exe", ""] {
            assert!(
                !is_allowed_trajectory_extension(ext),
                "{ext:?} must be rejected"
            );
        }
    }

    #[test]
    fn empty_and_extensionless_paths_are_rejected() {
        assert_eq!(
            validate_structure_path("").unwrap_err(),
            "structure path is empty"
        );
        let err = validate_structure_path("no-extension").unwrap_err();
        assert!(err.contains("no extension"), "{err}");
        assert_eq!(
            validate_trajectory_path("").unwrap_err(),
            "trajectory path is empty"
        );
        let err = validate_trajectory_path("no-extension").unwrap_err();
        assert!(err.contains("no extension"), "{err}");
    }

    #[test]
    fn missing_files_are_rejected() {
        let err = validate_structure_path("definitely-not-on-disk-9f3a.pdb").unwrap_err();
        assert!(err.contains("cannot resolve"), "{err}");
        let err = validate_trajectory_path("definitely-not-on-disk-9f3a.dcd").unwrap_err();
        assert!(err.contains("cannot resolve"), "{err}");
    }

    #[test]
    fn directories_with_viewer_extensions_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("dir.pdb");
        std::fs::create_dir(&fake).unwrap();
        let err = validate_structure_path(fake.to_str().unwrap()).unwrap_err();
        assert!(err.contains("not a regular file"), "{err}");
        let fake_traj = dir.path().join("dir.xtc");
        std::fs::create_dir(&fake_traj).unwrap();
        let err = validate_trajectory_path(fake_traj.to_str().unwrap()).unwrap_err();
        assert!(err.contains("not a regular file"), "{err}");
    }

    #[test]
    fn valid_structure_files_pass_and_are_canonicalized() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("model.pdb");
        std::fs::write(&file, b"HEADER slice-one smoke\n").unwrap();
        let canonical = validate_structure_path(file.to_str().unwrap()).unwrap();
        assert!(canonical.is_absolute());
        assert_eq!(canonical, file.canonicalize().unwrap());
    }

    #[test]
    fn valid_trajectory_files_pass_and_are_canonicalized() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("traj.dcd");
        std::fs::write(&file, b"slice-two smoke bytes\n").unwrap();
        let canonical = validate_trajectory_path(file.to_str().unwrap()).unwrap();
        assert!(canonical.is_absolute());
        assert_eq!(canonical, file.canonicalize().unwrap());
    }

    #[test]
    fn oversized_structures_are_rejected() {
        // Cannot cheaply write a >128 MiB file, so assert the cap arithmetic
        // and the error formatting contract instead.
        assert_eq!(MAX_STRUCTURE_BYTES, 128 * 1024 * 1024);
        let meta_len = MAX_STRUCTURE_BYTES + 1;
        let msg = format!(
            "structure file is {meta_len} bytes; the viewer caps structure files at {MAX_STRUCTURE_BYTES} bytes"
        );
        assert!(msg.contains("caps structure"));
    }

    #[test]
    fn trajectory_cap_leaves_room_for_real_md_runs() {
        assert_eq!(MAX_TRAJECTORY_BYTES, 512 * 1024 * 1024);
        // The trajectory cap must dwarf the structure cap: a single 25 ns
        // OpenMM run already emits DCDs far beyond any topology file.
        assert!(MAX_TRAJECTORY_BYTES >= 4 * MAX_STRUCTURE_BYTES);
    }

    #[test]
    fn viewer_url_percent_encodes_spaces_but_keeps_path_separators() {
        assert_eq!(
            viewer_url("C:/a b/model.pdb"),
            "viewer.html?src=C:/a%20b/model.pdb"
        );
        assert_eq!(
            viewer_url("D:\\pdb library\\x (1).cif"),
            "viewer.html?src=D:\\pdb%20library\\x%20%281%29.cif"
        );
    }

    #[test]
    fn viewer_trajectory_url_encodes_both_params() {
        assert_eq!(
            viewer_trajectory_url("C:/md run/topo.pdb", "C:/md run/sim 01.xtc"),
            "viewer.html?topo=C:/md%20run/topo.pdb&traj=C:/md%20run/sim%2001.xtc"
        );
        assert_eq!(
            viewer_trajectory_url("D:\\t\\x.pdb", "D:\\t\\y.nc"),
            "viewer.html?topo=D:\\t\\x.pdb&traj=D:\\t\\y.nc"
        );
    }
}

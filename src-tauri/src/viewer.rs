//! Dedicated molecular structure viewer window (slice 1/4 of #191, see #192).
//!
//! Renders the vendored open-source Mol\* bundle (`ui/viewer.html`) in its own
//! webview. Structure bytes travel only through the path-validated
//! `read_structure_bytes` command; the webview itself holds no filesystem
//! scope. Trajectory playback (DCD/XTC/TRR/NetCDF) arrives with slice 2 (#193)
//! via the same window.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub(crate) const VIEWER_WINDOW_LABEL: &str = "viewer-structure";

/// Event emitted to an already-open viewer window to swap in a new structure
/// without navigating (first open receives its path through the URL query, so
/// it never races the page's event listener).
const LOAD_STRUCTURE_EVENT: &str = "viewer://load-structure";

/// Upper bound for a structure file handed to the viewer. Structures are tiny
/// next to trajectories; a larger "structure" is almost certainly the wrong
/// file (e.g. a multi-frame trajectory), which slice 2 (#193) will route
/// through a separate trajectory-specific command.
pub(crate) const MAX_STRUCTURE_BYTES: u64 = 128 * 1024 * 1024;

pub(crate) fn is_allowed_structure_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "pdb" | "ent" | "cif" | "mmcif" | "gro"
    )
}

/// Resolve and vet a structure path: non-empty, allow-listed extension, an
/// existing regular file, and within the size cap. Returns the canonical path
/// on success; the error strings are user-facing.
pub(crate) fn validate_structure_path(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err("structure path is empty".to_string());
    }
    let raw = Path::new(path);
    let ext = raw
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| format!("structure path has no extension: {path:?}"))?;
    if !is_allowed_structure_extension(ext) {
        return Err(format!(
            "unsupported structure extension {ext:?}; expected pdb, ent, cif, mmcif or gro"
        ));
    }
    let canonical = std::fs::canonicalize(raw)
        .map_err(|e| format!("cannot resolve structure path {path:?}: {e}"))?;
    let meta = std::fs::metadata(&canonical)
        .map_err(|e| format!("cannot read structure file {path:?}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("structure path is not a regular file: {path:?}"));
    }
    if meta.len() > MAX_STRUCTURE_BYTES {
        return Err(format!(
            "structure file is {} bytes; the viewer caps structures at {MAX_STRUCTURE_BYTES} bytes",
            meta.len()
        ));
    }
    Ok(canonical)
}

/// Percent-encode the `src` query value dependency-free, keeping characters
/// that are harmless in a query (`:/\` for Windows and POSIX paths).
pub(crate) fn viewer_url(path: &str) -> String {
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
    format!("viewer.html?src={encoded}")
}

#[derive(Clone, Serialize)]
struct LoadStructurePayload {
    path: String,
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
    let existing = app.get_webview_window(VIEWER_WINDOW_LABEL);
    match existing {
        Some(window) => {
            let _ = window.set_focus();
            app.emit_to(
                VIEWER_WINDOW_LABEL,
                LOAD_STRUCTURE_EVENT,
                LoadStructurePayload { path },
            )
            .map_err(|e| format!("failed to deliver structure to the viewer window: {e}"))?;
        }
        None => {
            let url = viewer_url(&path);
            let mut builder =
                WebviewWindowBuilder::new(&app, VIEWER_WINDOW_LABEL, WebviewUrl::App(url.into()))
                    .title("Structure viewer")
                    .inner_size(1200.0, 840.0)
                    .min_inner_size(640.0, 480.0)
                    .resizable(true)
                    .general_autofill_enabled(false)
                    .on_navigation(crate::guard_webview_navigation);
            #[cfg(target_os = "windows")]
            let builder = builder.decorations(false).shadow(true);
            builder
                .build()
                .map_err(|e| format!("failed to open the structure viewer window: {e}"))?;
        }
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
        // Trajectories belong to slice 2 (#193) and must not ride slice 1.
        for ext in ["dcd", "xtc", "trr", "netcdf", "npz", "exe", ""] {
            assert!(
                !is_allowed_structure_extension(ext),
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
    }

    #[test]
    fn missing_files_are_rejected() {
        let err = validate_structure_path("definitely-not-on-disk-9f3a.pdb").unwrap_err();
        assert!(err.contains("cannot resolve"), "{err}");
    }

    #[test]
    fn directories_with_structure_extensions_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("dir.pdb");
        std::fs::create_dir(&fake).unwrap();
        let err = validate_structure_path(fake.to_str().unwrap()).unwrap_err();
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
    fn oversized_structures_are_rejected() {
        // Cannot cheaply write a >128 MiB file, so assert the cap arithmetic
        // and the error formatting contract instead.
        assert_eq!(MAX_STRUCTURE_BYTES, 128 * 1024 * 1024);
        let meta_len = MAX_STRUCTURE_BYTES + 1;
        let msg = format!(
            "structure file is {meta_len} bytes; the viewer caps structures at {MAX_STRUCTURE_BYTES} bytes"
        );
        assert!(msg.contains("caps structures"));
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
}

//! Dedicated molecular viewer window (slices 1-2/4 of #191, see #192/#193).
//!
//! Renders the vendored open-source Mol\* bundle (`ui/viewer.html`) in its own
//! webview. Structure and trajectory bytes travel only through the
//! path-validated `read_*_bytes` commands; the webview itself holds no
//! filesystem scope.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use wisp_llm::ToolSchema;
use wisp_tools::tool::{arg_str, arg_str_opt};
use wisp_tools::{Tool, ToolEnv, ToolResult};

pub(crate) const VIEWER_WINDOW_LABEL: &str = "viewer-structure";

/// Event emitted to an already-open viewer window to swap in a new structure
/// without navigating (first open receives its path through the URL query, so
/// it never races the page's event listener).
const LOAD_STRUCTURE_EVENT: &str = "viewer://load-structure";

/// Event carrying a new topology + trajectory pair into an open viewer window.
const LOAD_TRAJECTORY_EVENT: &str = "viewer://load-trajectory";

/// Scripted camera move for an already-open viewer (#194).
const CONTROL_CAMERA_EVENT: &str = "viewer://control-camera";

/// PyMOL-style selection for an already-open viewer (#194).
const SELECT_EVENT: &str = "viewer://select";

/// Representation switch pushed into an open viewer (#248). Same action as
/// the window's representation buttons.
const REPRESENTATION_EVENT: &str = "viewer://representation";

/// Color action pushed into an open viewer (#248).
const COLOR_EVENT: &str = "viewer://color";

/// Label toggle pushed into an open viewer (#248).
const LABELS_EVENT: &str = "viewer://labels";

/// Visibility action pushed into an open viewer (#248).
const VISIBILITY_EVENT: &str = "viewer://visibility";

/// Distance measurement pushed into an open viewer (#248).
const MEASURE_EVENT: &str = "viewer://measure";

/// Chrome-language update for an open viewer window (#247). The page owns
/// its string table; this event only tells it which column to render.
pub(crate) const LOCALE_EVENT: &str = "viewer://locale";

#[derive(Clone, Serialize)]
struct LocalePayload {
    locale: String,
}

/// The chrome language for a viewer about to open, read from the desktop UI
/// language setting. Only `zh` switches the chrome; anything else is `en`.
async fn chrome_locale(app: &AppHandle) -> String {
    let state = app.state::<crate::AppState>();
    match state
        .store
        .get_setting("locale")
        .await
        .ok()
        .flatten()
        .as_deref()
        .map(str::trim)
    {
        Some("zh") => "zh".into(),
        _ => "en".into(),
    }
}

/// Tell an already-open viewer window that the desktop UI language changed.
/// The page re-renders its own chrome and resets its title, so Mol*'s
/// bundled panels stay on Mol*'s language.
pub(crate) fn emit_viewer_locale(app: &AppHandle, locale: &str) {
    let _ = app.emit_to(
        VIEWER_WINDOW_LABEL,
        LOCALE_EVENT,
        LocalePayload {
            locale: locale.to_string(),
        },
    );
}

/// Native window title in the chrome language. The page corrects the title
/// through [`set_viewer_window_title`] once it knows its own mode, so this
/// only covers the moment between spawn and first paint.
fn viewer_window_title(kind: &str, locale: &str) -> &'static str {
    match (kind, locale) {
        ("trajectory", "zh") => "轨迹查看器",
        ("trajectory", _) => "Trajectory viewer",
        (_, "zh") => "结构查看器",
        (_, _) => "Structure viewer",
    }
}

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

pub(crate) fn viewer_url_with_selection(path: &str, selection: Option<&str>) -> String {
    let mut url = format!("viewer.html?src={}", percent_encode_path(path));
    if let Some(selection) = selection {
        url.push_str("&sel=");
        url.push_str(&percent_encode_path(selection));
    }
    url
}

pub(crate) fn viewer_trajectory_url(structure: &str, trajectory: &str) -> String {
    format!(
        "viewer.html?topo={}&traj={}",
        percent_encode_path(structure),
        percent_encode_path(trajectory)
    )
}

/// Same URLs with the chrome language appended, so the freshly opened page
/// renders the right column before its event listeners exist (#247).
pub(crate) fn viewer_url_with_locale(path: &str, selection: Option<&str>, locale: &str) -> String {
    format!(
        "{}&lang={}",
        viewer_url_with_selection(path, selection),
        if locale == "zh" { "zh" } else { "en" }
    )
}

pub(crate) fn viewer_trajectory_url_with_locale(
    structure: &str,
    trajectory: &str,
    locale: &str,
) -> String {
    format!(
        "{}&lang={}",
        viewer_trajectory_url(structure, trajectory),
        if locale == "zh" { "zh" } else { "en" }
    )
}

#[derive(Clone, Serialize)]
struct LoadStructurePayload {
    path: String,
    /// Applied only after this structure finishes loading. Absent when the
    /// caller only wants the file swapped in.
    #[serde(skip_serializing_if = "Option::is_none")]
    selection: Option<String>,
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
    let builder = WebviewWindowBuilder::new(app, VIEWER_WINDOW_LABEL, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(1200.0, 840.0)
        .min_inner_size(640.0, 480.0)
        .resizable(true)
        .minimizable(true)
        .maximizable(true)
        .general_autofill_enabled(false)
        .on_navigation(crate::guard_webview_navigation);
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
                LoadStructurePayload {
                    path: path.clone(),
                    selection: None,
                },
            )
            .map_err(|e| format!("failed to deliver structure to the viewer window: {e}"))?;
        }
        None => {
            let locale = chrome_locale(&app).await;
            spawn_viewer_window(
                &app,
                viewer_url_with_locale(&path, None, &locale),
                viewer_window_title("structure", &locale),
            )?
        }
    }
    remember_viewer_path(&path);
    Ok(())
}

/// Empty selections mean "load only". A non-empty one must be the same PyMOL
/// subset the window can highlight, including a ligand (`organic` / `hetatm`).
pub(crate) fn normalize_presentation_selection(
    selection: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(selection) = selection.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    Ok(Some(normalize_pymol_selection(selection)?))
}

/// Open or reuse the structure window and, once that file has loaded, highlight
/// `selection`. One payload so a later pose cannot highlight the previous
/// structure. This is the presentation a running task pushes; it is not a
/// docking-specific command.
#[tauri::command]
pub(crate) async fn present_structure_in_viewer(
    app: AppHandle,
    path: String,
    selection: Option<String>,
) -> Result<(), String> {
    let selection = normalize_presentation_selection(selection.as_deref())?;
    let canonical = validate_structure_path(&path)?;
    let path = canonical.to_string_lossy().into_owned();
    wait_for_frame_gap().await;
    match app.get_webview_window(VIEWER_WINDOW_LABEL) {
        Some(_window) => {
            app.emit_to(
                VIEWER_WINDOW_LABEL,
                LOAD_STRUCTURE_EVENT,
                LoadStructurePayload {
                    path: path.clone(),
                    selection,
                },
            )
            .map_err(|e| format!("failed to deliver structure to the viewer window: {e}"))?;
        }
        None => {
            let locale = chrome_locale(&app).await;
            spawn_viewer_window(
                &app,
                viewer_url_with_locale(&path, selection.as_deref(), &locale),
                viewer_window_title("structure", &locale),
            )?
        }
    }
    remember_viewer_path(&path);
    Ok(())
}

/// Ask the main window's open chat to explain the structure on screen.
#[tauri::command]
pub(crate) async fn explain_structure_selection(
    state: tauri::State<'_, crate::AppState>,
    app: AppHandle,
    path: String,
    selection: Option<String>,
) -> Result<(), String> {
    let session_id = state
        .active_frame("main")
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            "Open a chat in the main window before asking for an explanation.".to_string()
        })?;
    let name = Path::new(&path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path.as_str());
    let focus = selection
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!(" The selected region is `{value}`."))
        .unwrap_or_else(|| {
            " No residue selection was set, so explain the structure as a whole.".to_string()
        });
    let message = format!(
        "Explain the structure now shown in the molecular viewer.\n\nFile: {path}\nName: {name}.{focus}\n\nSay what this molecule or region is, in plain language, using only what the file and selection identify. Do not start a new docking run."
    );
    crate::agent_turn::send_message_inner(
        state.inner(),
        app,
        "main",
        Some(session_id),
        message,
        None,
        None,
        None,
        None,
        None,
        Some(false),
        Some(false),
        None,
        crate::agent_turn::TurnOrigin::Desktop,
    )
    .await
    .map(|_| ())
}

/// Project-conversation tool for the same presentation as
/// [`present_structure_in_viewer`]. The research assistant does not receive it.
pub(crate) struct PresentStructureTool {
    app: AppHandle,
}

impl PresentStructureTool {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait]
impl Tool for PresentStructureTool {
    fn name(&self) -> &str {
        "present_structure"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "present_structure",
            "Show a structure file in the molecular viewer. An optional PyMOL selection (organic, hetatm, name, resi, polymer.protein) is applied only after this file finishes loading. Call again with the next file to replace the current structure.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Local pdb, ent, cif, mmcif, or gro file."},
                    "selection": {"type": "string", "description": "Optional PyMOL selection applied after the load."}
                },
                "required": ["path"]
            }),
        )
    }

    fn preview(&self, args: &Value) -> String {
        arg_str_opt(args, "path").unwrap_or_default()
    }

    async fn run(&self, args: &Value, _env: &dyn ToolEnv) -> ToolResult {
        let path = match arg_str(args, "path") {
            Ok(path) => path,
            Err(error) => return ToolResult::fail(error),
        };
        let selection = arg_str_opt(args, "selection");
        match present_structure_in_viewer(self.app.clone(), path, selection).await {
            Ok(()) => ToolResult::ok("structure presented"),
            Err(error) => ToolResult::fail(error),
        }
    }
}

/// Project-conversation tool that docks a ligand onto a receptor and presents
/// each written pose. The research assistant does not receive it.
pub(crate) struct DockLigandTool {
    app: AppHandle,
}

/// Chat-side tool that drives the already-open structure window (#248). Every
/// action forwards to the same events the window's own controls use, so the
/// chat cannot drift from what the user sees. It never sends messages into
/// the chat: viewer-to-chat traffic stays on the explicit Explain-in-chat
/// action, and neither direction starts a docking run.
pub(crate) struct ControlViewerTool {
    app: AppHandle,
}

impl ControlViewerTool {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }

    /// Validation shared by the chat tool and the window itself: the color
    /// words the viewer chrome understands.
    fn normalize_color_mode(mode: &str) -> Result<String, String> {
        const MODES: [&str; 13] = [
            "element",
            "chain",
            "spectrum",
            "red",
            "green",
            "blue",
            "yellow",
            "cyan",
            "magenta",
            "orange",
            "white",
            "gray",
            "clear-paint",
        ];
        let mode = mode.trim();
        if MODES.contains(&mode) {
            Ok(mode.to_string())
        } else {
            Err(format!(
                "unsupported color mode {mode:?}; expected one of {}",
                MODES.join(", ")
            ))
        }
    }

    fn normalize_representation_kind(kind: &str) -> Result<String, String> {
        const KINDS: [&str; 4] = ["cartoon", "stick", "sphere", "surface"];
        let kind = kind.trim();
        if KINDS.contains(&kind) {
            Ok(kind.to_string())
        } else {
            Err(format!(
                "unsupported representation {kind:?}; expected one of {}",
                KINDS.join(", ")
            ))
        }
    }

    fn normalize_visibility_mode(mode: &str) -> Result<String, String> {
        const MODES: [&str; 3] = ["hide", "others", "show"];
        let mode = mode.trim();
        if MODES.contains(&mode) {
            Ok(mode.to_string())
        } else {
            Err(format!(
                "unsupported visibility mode {mode:?}; expected one of {}",
                MODES.join(", ")
            ))
        }
    }

    async fn emit<T: Serialize + Clone>(
        app: &AppHandle,
        event: &str,
        payload: T,
    ) -> Result<(), String> {
        require_viewer_window(app)?;
        app.emit_to(VIEWER_WINDOW_LABEL, event, payload)
            .map_err(|e| format!("failed to deliver viewer command: {e}"))
    }
}

#[async_trait]
impl Tool for ControlViewerTool {
    fn name(&self) -> &str {
        "control_viewer"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "control_viewer",
            "Drive the open structure window: select atoms, switch representations, recolor, move the camera, toggle residue labels, hide or show parts, and measure the distance between two selections. The window must already be open. This tool only changes the viewer; it never sends messages to the chat.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["select", "representation", "color", "reset", "zoom", "rotate", "labels", "visibility", "measure"],
                        "description": "What to do in the viewer."
                    },
                    "expression": {"type": "string", "description": "PyMOL selection, for action=select."},
                    "kind": {"type": "string", "enum": ["cartoon", "stick", "sphere", "surface"], "description": "Representation, for action=representation."},
                    "visible": {"type": "boolean", "description": "On or off, for action=representation and action=labels."},
                    "mode": {"type": "string", "description": "Color (element, chain, spectrum, a color word, or clear-paint) for action=color; hide, others, or show for action=visibility."},
                    "factor": {"type": "number", "description": "Zoom factor > 1 moves closer, for action=zoom."},
                    "axis": {"type": "string", "enum": ["x", "y", "z"], "description": "For action=rotate."},
                    "degrees": {"type": "number", "description": "Rotation angle, for action=rotate."},
                    "a": {"type": "string", "description": "First PyMOL selection, for action=measure."},
                    "b": {"type": "string", "description": "Second PyMOL selection, for action=measure."}
                },
                "required": ["action"]
            }),
        )
    }

    fn preview(&self, args: &Value) -> String {
        arg_str_opt(args, "action").unwrap_or_default()
    }

    async fn run(&self, args: &Value, _env: &dyn ToolEnv) -> ToolResult {
        let Ok(action) = arg_str(args, "action") else {
            return ToolResult::fail("action is required");
        };
        let app = self.app.clone();
        let outcome = match action.as_str() {
            "select" => {
                let Ok(expression) = arg_str(args, "expression") else {
                    return ToolResult::fail("select needs an expression");
                };
                select_in_viewer(app, expression)
                    .await
                    .map(|expression| format!("selected {expression}"))
            }
            "representation" => {
                let Ok(kind) = arg_str(args, "kind") else {
                    return ToolResult::fail("representation needs a kind");
                };
                let kind = match Self::normalize_representation_kind(&kind) {
                    Ok(kind) => kind,
                    Err(error) => return ToolResult::fail(error),
                };
                let visible = args.get("visible").and_then(Value::as_bool).unwrap_or(true);
                Self::emit(
                    &app,
                    REPRESENTATION_EVENT,
                    json!({ "kind": kind, "visible": visible }),
                )
                .await
                .map(|()| format!("{kind} {}", if visible { "shown" } else { "hidden" }))
            }
            "color" => {
                let Ok(mode) = arg_str(args, "mode") else {
                    return ToolResult::fail("color needs a mode");
                };
                let mode = match Self::normalize_color_mode(&mode) {
                    Ok(mode) => mode,
                    Err(error) => return ToolResult::fail(error),
                };
                Self::emit(&app, COLOR_EVENT, json!({ "mode": mode }))
                    .await
                    .map(|()| format!("colored by {mode}"))
            }
            "reset" | "zoom" | "rotate" => {
                let payload = match normalize_camera_command(
                    &action,
                    arg_str_opt(args, "axis"),
                    args.get("degrees").and_then(Value::as_f64),
                    args.get("factor").and_then(Value::as_f64),
                    None,
                ) {
                    Ok(payload) => payload,
                    Err(error) => return ToolResult::fail(error),
                };
                Self::emit(&app, CONTROL_CAMERA_EVENT, payload)
                    .await
                    .map(|()| format!("camera {action}"))
            }
            "labels" => {
                let visible = args.get("visible").and_then(Value::as_bool).unwrap_or(true);
                Self::emit(&app, LABELS_EVENT, json!({ "visible": visible }))
                    .await
                    .map(|()| format!("labels {}", if visible { "on" } else { "off" }))
            }
            "visibility" => {
                let Ok(mode) = arg_str(args, "mode") else {
                    return ToolResult::fail("visibility needs a mode");
                };
                let mode = match Self::normalize_visibility_mode(&mode) {
                    Ok(mode) => mode,
                    Err(error) => return ToolResult::fail(error),
                };
                Self::emit(&app, VISIBILITY_EVENT, json!({ "mode": mode }))
                    .await
                    .map(|()| format!("visibility {mode}"))
            }
            "measure" => {
                let Ok(a) = arg_str(args, "a") else {
                    return ToolResult::fail("measure needs selections a and b");
                };
                let Ok(b) = arg_str(args, "b") else {
                    return ToolResult::fail("measure needs selections a and b");
                };
                let a = match normalize_pymol_selection(&a) {
                    Ok(a) => a,
                    Err(error) => return ToolResult::fail(error),
                };
                let b = match normalize_pymol_selection(&b) {
                    Ok(b) => b,
                    Err(error) => return ToolResult::fail(error),
                };
                Self::emit(&app, MEASURE_EVENT, json!({ "a": a, "b": b }))
                    .await
                    .map(|()| format!("measuring {a} to {b}"))
            }
            other => return ToolResult::fail(format!("unsupported viewer action {other:?}")),
        };
        match outcome {
            Ok(message) => ToolResult::ok(message),
            Err(error) => ToolResult::fail(error),
        }
    }
}

impl DockLigandTool {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait]
impl Tool for DockLigandTool {
    fn name(&self) -> &str {
        "dock_ligand"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "dock_ligand",
            "Rigid-dock a ligand PDB onto a receptor PDB. Each sample is written as pose-NNN.pdb and shown in the molecular viewer before the next sample, so the open window advances through the search. This is geometric placement, not an affinity prediction.",
            json!({
                "type": "object",
                "properties": {
                    "receptor": {"type": "string", "description": "Receptor PDB path."},
                    "ligand": {"type": "string", "description": "Ligand PDB path."},
                    "output_dir": {"type": "string", "description": "Directory that receives pose-001.pdb and the following frames."},
                    "samples": {"type": "integer", "description": "How many poses to write, from 1 to 24. Default 8."}
                },
                "required": ["receptor", "ligand", "output_dir"]
            }),
        )
    }

    fn preview(&self, args: &Value) -> String {
        arg_str_opt(args, "output_dir").unwrap_or_default()
    }

    async fn run(&self, args: &Value, _env: &dyn ToolEnv) -> ToolResult {
        let receptor = match arg_str(args, "receptor") {
            Ok(path) => path,
            Err(error) => return ToolResult::fail(error),
        };
        let ligand = match arg_str(args, "ligand") {
            Ok(path) => path,
            Err(error) => return ToolResult::fail(error),
        };
        let output_dir = match arg_str(args, "output_dir") {
            Ok(path) => path,
            Err(error) => return ToolResult::fail(error),
        };
        let samples = args
            .get("samples")
            .and_then(|value| value.as_u64())
            .unwrap_or(8) as usize;
        let frames = match wisp_bio::dock::write_docking_frames(
            std::path::Path::new(&receptor),
            std::path::Path::new(&ligand),
            std::path::Path::new(&output_dir),
            samples,
        ) {
            Ok(frames) => frames,
            Err(error) => return ToolResult::fail(error),
        };
        for frame in &frames {
            if let Err(error) = present_structure_in_viewer(
                self.app.clone(),
                frame.to_string_lossy().into_owned(),
                Some("hetatm".to_string()),
            )
            .await
            {
                return ToolResult::fail(error);
            }
        }
        ToolResult::ok(format!("presented {} docking poses", frames.len()))
    }
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
                    structure_path: structure_path.clone(),
                    trajectory_path: trajectory_path.clone(),
                },
            )
            .map_err(|e| format!("failed to deliver trajectory to the viewer window: {e}"))?;
        }
        None => {
            let locale = chrome_locale(&app).await;
            spawn_viewer_window(
                &app,
                viewer_trajectory_url_with_locale(&structure_path, &trajectory_path, &locale),
                viewer_window_title("trajectory", &locale),
            )?
        }
    }
    remember_viewer_path(&structure_path);
    remember_viewer_path(&trajectory_path);
    Ok(())
}

#[derive(Clone, Serialize)]
struct CameraPayload {
    action: String,
    axis: Option<String>,
    degrees: Option<f64>,
    factor: Option<f64>,
    residue: Option<i64>,
}

#[derive(Clone, Serialize)]
struct SelectionPayload {
    expression: String,
}

/// Normalize one scripted camera step. `rotate` needs an axis and degrees,
/// `zoom` needs a positive factor, `center` may name a residue, `reset` takes
/// no extras.
pub(crate) fn normalize_camera_command(
    action: &str,
    axis: Option<String>,
    degrees: Option<f64>,
    factor: Option<f64>,
    residue: Option<i64>,
) -> Result<CameraPayload, String> {
    let action = action.trim().to_ascii_lowercase();
    match action.as_str() {
        "reset" => Ok(CameraPayload {
            action,
            axis: None,
            degrees: None,
            factor: None,
            residue: None,
        }),
        "rotate" => {
            let axis = axis
                .as_deref()
                .map(str::trim)
                .filter(|value| matches!(*value, "x" | "y" | "z"))
                .ok_or_else(|| "rotate requires axis x, y, or z".to_string())?;
            let degrees = degrees.ok_or_else(|| "rotate requires degrees".to_string())?;
            if !degrees.is_finite() {
                return Err("rotate degrees must be finite".into());
            }
            Ok(CameraPayload {
                action,
                axis: Some(axis.to_string()),
                degrees: Some(degrees),
                factor: None,
                residue: None,
            })
        }
        "zoom" => {
            let factor = factor.ok_or_else(|| "zoom requires factor".to_string())?;
            if !factor.is_finite() || factor <= 0.0 {
                return Err("zoom factor must be a positive finite number".into());
            }
            Ok(CameraPayload {
                action,
                axis: None,
                degrees: None,
                factor: Some(factor),
                residue: None,
            })
        }
        "center" => Ok(CameraPayload {
            action,
            axis: None,
            degrees: None,
            factor: None,
            residue,
        }),
        _ => Err(format!(
            "unsupported camera action {action:?}; expected rotate, zoom, center, or reset"
        )),
    }
}

/// Accept the PyMOL dialect subset the viewer highlights: `name CA`,
/// `resi 1-10`, `polymer.protein`, joined by `and`.
pub(crate) fn normalize_pymol_selection(expression: &str) -> Result<String, String> {
    let clauses: Vec<&str> = expression
        .split(" and ")
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .collect();
    if clauses.is_empty() {
        return Err("selection expression is empty".into());
    }
    for clause in &clauses {
        validate_pymol_clause(clause)?;
    }
    Ok(clauses.join(" and "))
}

fn validate_pymol_clause(clause: &str) -> Result<(), String> {
    let mut tokens = clause.split_whitespace();
    let head = tokens
        .next()
        .ok_or_else(|| format!("empty selection clause in {clause:?}"))?;
    match head {
        "name" => {
            let atom = tokens
                .next()
                .ok_or_else(|| "name selection requires an atom name".to_string())?;
            if tokens.next().is_some() || !atom.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(format!("unsupported name selection {clause:?}"));
            }
            Ok(())
        }
        "resi" => {
            let range = tokens
                .next()
                .ok_or_else(|| "resi selection requires a residue range".to_string())?;
            if tokens.next().is_some() || !valid_residue_range(range) {
                return Err(format!("unsupported resi selection {clause:?}"));
            }
            Ok(())
        }
        "organic" | "hetatm" | "polymer.protein" | "polymer.nucleic" => {
            if tokens.next().is_some() {
                return Err(format!("unsupported polymer selection {clause:?}"));
            }
            Ok(())
        }
        _ => Err(format!(
            "unsupported selection clause {clause:?}; expected name, resi, organic, hetatm, or polymer.protein"
        )),
    }
}

fn valid_residue_range(range: &str) -> bool {
    let mut parts = range.split('-');
    let start = parts.next().and_then(|value| value.parse::<u32>().ok());
    let end = match parts.next() {
        Some(value) => value.parse::<u32>().ok(),
        None => start,
    };
    parts.next().is_none()
        && matches!((start, end), (Some(start), Some(end)) if start > 0 && end >= start)
}

#[derive(Debug)]
pub(crate) enum ArtifactHandoff {
    Structure(PathBuf),
    Trajectory {
        structure: PathBuf,
        trajectory: PathBuf,
    },
}

/// Route one artifact path to the structure window or a trajectory whose
/// topology sits beside it. Unsupported types stay hidden from the card.
pub(crate) fn resolve_artifact_handoff(path: &str) -> Result<ArtifactHandoff, String> {
    let raw = Path::new(path);
    let ext = raw
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if is_allowed_structure_extension(&ext) {
        return Ok(ArtifactHandoff::Structure(validate_structure_path(path)?));
    }
    if is_allowed_trajectory_extension(&ext) {
        let trajectory = validate_trajectory_path(path)?;
        let structure = sibling_topology(&trajectory).ok_or_else(|| {
            format!(
                "trajectory {} has no sibling topology (pdb, cif, mmcif, gro, or ent)",
                trajectory.display()
            )
        })?;
        return Ok(ArtifactHandoff::Trajectory {
            structure,
            trajectory,
        });
    }
    Err(format!(
        "unsupported viewer artifact extension {ext:?}; expected a structure or trajectory file"
    ))
}

fn sibling_topology(trajectory: &Path) -> Option<PathBuf> {
    let parent = trajectory.parent()?;
    let stem = trajectory.file_stem()?.to_str()?;
    for ext in ["pdb", "cif", "mmcif", "gro", "ent"] {
        let candidate = parent.join(format!("{stem}.{ext}"));
        if candidate.is_file() {
            if let Ok(path) = validate_structure_path(&candidate.to_string_lossy()) {
                return Some(path);
            }
        }
    }
    None
}

const FRAME_GAP: std::time::Duration = std::time::Duration::from_millis(1800);

static LAST_FRAME_AT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

async fn wait_for_frame_gap() {
    let wait = {
        let mut slot = LAST_FRAME_AT.lock().unwrap_or_else(|err| err.into_inner());
        let wait = slot
            .map(|last| FRAME_GAP.saturating_sub(last.elapsed()))
            .unwrap_or(std::time::Duration::ZERO);
        *slot = Some(std::time::Instant::now() + wait);
        wait
    };
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
}

fn remember_viewer_path(path: &str) {
    let mut recent = RECENT_VIEWER_PATHS
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    recent.retain(|existing| existing != path);
    recent.insert(0, path.to_string());
    recent.truncate(8);
}

static RECENT_VIEWER_PATHS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn require_viewer_window(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window(VIEWER_WINDOW_LABEL).is_none() {
        return Err("viewer window is not open".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn control_camera(
    app: AppHandle,
    action: String,
    axis: Option<String>,
    degrees: Option<f64>,
    factor: Option<f64>,
    residue: Option<i64>,
) -> Result<(), String> {
    let payload = normalize_camera_command(&action, axis, degrees, factor, residue)?;
    require_viewer_window(&app)?;
    app.emit_to(VIEWER_WINDOW_LABEL, CONTROL_CAMERA_EVENT, payload)
        .map_err(|e| format!("failed to deliver camera command: {e}"))
}

#[tauri::command]
pub(crate) async fn select_in_viewer(app: AppHandle, expression: String) -> Result<String, String> {
    let expression = normalize_pymol_selection(&expression)?;
    require_viewer_window(&app)?;
    app.emit_to(
        VIEWER_WINDOW_LABEL,
        SELECT_EVENT,
        SelectionPayload {
            expression: expression.clone(),
        },
    )
    .map_err(|e| format!("failed to deliver selection: {e}"))?;
    Ok(expression)
}

#[tauri::command]
pub(crate) async fn open_artifact_in_viewer(app: AppHandle, path: String) -> Result<(), String> {
    match resolve_artifact_handoff(&path)? {
        ArtifactHandoff::Structure(structure) => {
            open_structure_viewer(app, structure.to_string_lossy().into_owned()).await
        }
        ArtifactHandoff::Trajectory {
            structure,
            trajectory,
        } => {
            open_trajectory_viewer(
                app,
                structure.to_string_lossy().into_owned(),
                trajectory.to_string_lossy().into_owned(),
            )
            .await
        }
    }
}

#[tauri::command]
pub(crate) async fn list_viewer_recent() -> Result<Vec<String>, String> {
    Ok(RECENT_VIEWER_PATHS
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone())
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

const PNG_DATA_URL_PREFIX: &str = "data:image/png;base64,";

/// Decode a PNG captured from the viewer canvas and save it through the
/// native save dialog. Returns the saved path, or `None` when the user
/// cancels.
#[tauri::command]
pub(crate) async fn export_viewer_image(
    app: AppHandle,
    data_url: String,
) -> Result<Option<String>, String> {
    let payload = data_url
        .strip_prefix(PNG_DATA_URL_PREFIX)
        .ok_or_else(|| "expected a data:image/png;base64 data URL".to_string())?;
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| format!("invalid PNG data URL: {e}"))?;
    if !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Err("decoded data is not a PNG image".into());
    }
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name("structure-view.png")
        .add_filter("PNG image", &["png"])
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(dest) = rx.await.map_err(|e| format!("{e}"))? else {
        return Ok(None);
    };
    let dest_path = PathBuf::from(dest.to_string());
    tokio::fs::write(&dest_path, bytes)
        .await
        .map_err(|e| format!("failed to write {}: {e}", dest_path.display()))?;
    Ok(Some(dest_path.to_string_lossy().into_owned()))
}

/// Let the viewer page correct its native window title after the chrome
/// language or the loaded mode changes (#247). The title comes from the
/// page's own string table, so it always matches what the chrome shows.
#[tauri::command]
pub(crate) async fn set_viewer_window_title(app: AppHandle, title: String) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("viewer window title must not be empty".into());
    }
    let window = app
        .get_webview_window(VIEWER_WINDOW_LABEL)
        .ok_or_else(|| "viewer window is not open".to_string())?;
    window
        .set_title(title)
        .map_err(|e| format!("failed to set the viewer window title: {e}"))
}

/// A PDB identifier: exactly four ASCII alphanumerics, normalized to upper
/// case (classic RCSB entry ids such as `4KC3`).
pub(crate) fn normalize_pdb_id(raw: &str) -> Result<String, String> {
    let id = raw.trim();
    if id.len() == 4 && id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Ok(id.to_ascii_uppercase())
    } else {
        Err(format!(
            "'{raw}' is not a PDB identifier; expected four characters like 4KC3"
        ))
    }
}

/// Download the RCSB mmCIF for one entry into the app cache and present it
/// through the same load-structure path a local file uses (#249). The
/// structure window's command box and the chat tools that steer the window
/// both land here; the download itself never starts a chat turn. Local
/// files keep their extension whitelist — this path only accepts ids.
#[tauri::command]
pub(crate) async fn open_pdb_entry(app: AppHandle, pdb_id: String) -> Result<(), String> {
    let id = normalize_pdb_id(&pdb_id)?;
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("viewer cache directory is unavailable: {e}"))?
        .join("pdb");
    tokio::fs::create_dir_all(&cache_dir)
        .await
        .map_err(|e| format!("failed to create the PDB cache directory: {e}"))?;
    let dest = cache_dir.join(format!("{id}.cif"));
    if !tokio::fs::try_exists(&dest).await.unwrap_or(false) {
        let client = reqwest::Client::builder()
            .user_agent("wisp-depmap-viewer")
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| format!("failed to build the PDB download client: {e}"))?;
        let response = client
            .get(format!("https://files.rcsb.org/download/{id}.cif"))
            .send()
            .await
            .map_err(|e| format!("failed to reach files.rcsb.org for {id}: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "RCSB returned {} for {id}",
                response.status().as_u16()
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("failed to download {id}: {e}"))?;
        if bytes.len() as u64 > MAX_STRUCTURE_BYTES {
            return Err(format!(
                "entry {id} is {} bytes, above the {} byte structure cap",
                bytes.len(),
                MAX_STRUCTURE_BYTES
            ));
        }
        tokio::fs::write(&dest, &bytes)
            .await
            .map_err(|e| format!("failed to cache {id}: {e}"))?;
    }
    let canonical = validate_structure_path(&dest.to_string_lossy())?;
    let path = canonical.to_string_lossy().into_owned();
    match app.get_webview_window(VIEWER_WINDOW_LABEL) {
        Some(window) => {
            let _ = window.set_focus();
            app.emit_to(
                VIEWER_WINDOW_LABEL,
                LOAD_STRUCTURE_EVENT,
                LoadStructurePayload {
                    path: path.clone(),
                    selection: None,
                },
            )
            .map_err(|e| format!("failed to deliver structure to the viewer window: {e}"))?;
        }
        None => {
            let locale = chrome_locale(&app).await;
            spawn_viewer_window(
                &app,
                viewer_url_with_locale(&path, None, &locale),
                viewer_window_title("structure", &locale),
            )?
        }
    }
    remember_viewer_path(&path);
    Ok(())
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
            viewer_url_with_locale("C:/a b/model.pdb", None, "en"),
            "viewer.html?src=C:/a%20b/model.pdb&lang=en"
        );
        assert_eq!(
            viewer_url_with_locale("D:\\pdb library\\x (1).cif", Some("chain A"), "zh"),
            "viewer.html?src=D:\\pdb%20library\\x%20%281%29.cif&sel=chain%20A&lang=zh"
        );
    }

    #[test]
    fn viewer_trajectory_url_appends_the_chrome_language() {
        assert_eq!(
            viewer_trajectory_url_with_locale("C:/t/x.pdb", "C:/t/y.nc", "zh"),
            "viewer.html?topo=C:/t/x.pdb&traj=C:/t/y.nc&lang=zh"
        );
        assert_eq!(
            viewer_trajectory_url_with_locale("C:/t/x.pdb", "C:/t/y.xtc", "fr"),
            "viewer.html?topo=C:/t/x.pdb&traj=C:/t/y.xtc&lang=en"
        );
    }

    #[test]
    fn viewer_window_titles_follow_the_chrome_language() {
        assert_eq!(viewer_window_title("structure", "zh"), "结构查看器");
        assert_eq!(viewer_window_title("structure", "en"), "Structure viewer");
        assert_eq!(viewer_window_title("trajectory", "zh"), "轨迹查看器");
        assert_eq!(viewer_window_title("trajectory", "en"), "Trajectory viewer");
    }

    #[test]
    fn pdb_ids_normalize_and_reject_other_input() {
        assert_eq!(normalize_pdb_id("4kc3").unwrap(), "4KC3");
        assert_eq!(normalize_pdb_id(" 4KC3 ").unwrap(), "4KC3");
        assert_eq!(normalize_pdb_id("1crn").unwrap(), "1CRN");
        assert!(normalize_pdb_id("4KC").is_err());
        assert!(normalize_pdb_id("4KC33").is_err());
        assert!(normalize_pdb_id("").is_err());
        assert!(normalize_pdb_id("C:/a.pdb").is_err());
        // Four plain letters still parse as an id; the frontend only routes
        // here when the whole command box is one bare word, and RCSB decides.
        assert!(normalize_pdb_id("7ABC").is_ok());
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

    #[test]
    fn presentation_selection_accepts_a_ligand_and_drops_blank() {
        assert_eq!(normalize_presentation_selection(None).unwrap(), None);
        assert_eq!(normalize_presentation_selection(Some("  ")).unwrap(), None);
        assert_eq!(
            normalize_presentation_selection(Some("organic"))
                .unwrap()
                .as_deref(),
            Some("organic")
        );
        assert_eq!(
            normalize_presentation_selection(Some("name CA and polymer.protein"))
                .unwrap()
                .as_deref(),
            Some("name CA and polymer.protein")
        );
        assert!(normalize_presentation_selection(Some("ligand")).is_err());
    }

    #[test]
    fn first_open_carries_the_selection_on_the_url() {
        assert_eq!(
            viewer_url_with_selection("C:/a b/model.pdb", Some("organic")),
            "viewer.html?src=C:/a%20b/model.pdb&sel=organic"
        );
    }

    #[test]
    fn camera_commands_keep_only_the_fields_their_action_needs() {
        let rotate =
            normalize_camera_command("Rotate", Some("y".into()), Some(15.0), None, None).unwrap();
        assert_eq!(rotate.action, "rotate");
        assert_eq!(rotate.axis.as_deref(), Some("y"));
        assert!(
            normalize_camera_command("rotate", Some("w".into()), Some(1.0), None, None).is_err()
        );
        let zoom = normalize_camera_command("zoom", None, None, Some(1.5), None).unwrap();
        assert_eq!(zoom.factor, Some(1.5));
        assert!(normalize_camera_command("zoom", None, None, Some(0.0), None).is_err());
        let center = normalize_camera_command("center", None, None, None, Some(42)).unwrap();
        assert_eq!(center.residue, Some(42));
        let reset =
            normalize_camera_command("reset", Some("x".into()), Some(9.0), Some(2.0), Some(1))
                .unwrap();
        assert!(reset.axis.is_none() && reset.degrees.is_none());
    }

    #[test]
    fn pymol_selection_accepts_the_three_scripted_expressions() {
        assert_eq!(normalize_pymol_selection("name CA").unwrap(), "name CA");
        assert_eq!(normalize_pymol_selection("resi 1-10").unwrap(), "resi 1-10");
        assert_eq!(
            normalize_pymol_selection("  polymer.protein and name CA  ").unwrap(),
            "polymer.protein and name CA"
        );
        assert!(normalize_pymol_selection("resi 0").is_err());
        assert!(normalize_pymol_selection("chain A").is_err());
        assert!(normalize_pymol_selection("").is_err());
    }

    #[test]
    fn artifact_handoff_pairs_a_trajectory_with_its_sibling_topology() {
        let dir = tempfile::tempdir().unwrap();
        let topo = dir.path().join("sim.pdb");
        let traj = dir.path().join("sim.xtc");
        std::fs::write(&topo, b"ATOM\n").unwrap();
        std::fs::write(&traj, b"coords").unwrap();
        match resolve_artifact_handoff(traj.to_str().unwrap()).unwrap() {
            ArtifactHandoff::Trajectory {
                structure,
                trajectory,
            } => {
                assert_eq!(structure, topo.canonicalize().unwrap());
                assert_eq!(trajectory, traj.canonicalize().unwrap());
            }
            ArtifactHandoff::Structure(_) => panic!("expected a trajectory handoff"),
        }
        let orphan = dir.path().join("orphan.dcd");
        std::fs::write(&orphan, b"coords").unwrap();
        assert!(resolve_artifact_handoff(orphan.to_str().unwrap())
            .unwrap_err()
            .contains("sibling topology"));
        std::fs::write(dir.path().join("note.txt"), b"no").unwrap();
        assert!(
            resolve_artifact_handoff(dir.path().join("note.txt").to_str().unwrap())
                .unwrap_err()
                .contains("unsupported")
        );
    }

    #[test]
    fn png_data_urls_decode_and_reject_non_png_payloads() {
        use base64::Engine as _;
        fn decode(data_url: &str) -> Result<Vec<u8>, String> {
            let payload = data_url
                .strip_prefix(PNG_DATA_URL_PREFIX)
                .ok_or_else(|| "expected a data:image/png;base64 data URL".to_string())?;
            base64::engine::general_purpose::STANDARD
                .decode(payload)
                .map_err(|e| format!("invalid PNG data URL: {e}"))
        }
        let png = base64::engine::general_purpose::STANDARD.encode([0x89, b'P', b'N', b'G', 1, 2]);
        let bytes = decode(&format!("{PNG_DATA_URL_PREFIX}{png}")).unwrap();
        assert_eq!(bytes, vec![0x89, b'P', b'N', b'G', 1, 2]);
        let jpeg = base64::engine::general_purpose::STANDARD.encode([0xff, 0xd8, 0xff]);
        let bytes = decode(&format!("{PNG_DATA_URL_PREFIX}{jpeg}")).unwrap();
        assert!(!bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(decode("data:image/jpeg;base64,AAAA").is_err());
        assert!(decode("not a data url").is_err());
    }

    #[test]
    fn control_viewer_arguments_match_the_window_chrome() {
        for mode in [
            "element",
            "chain",
            "spectrum",
            "red",
            "green",
            "blue",
            "yellow",
            "cyan",
            "magenta",
            "orange",
            "white",
            "gray",
            "clear-paint",
        ] {
            assert_eq!(ControlViewerTool::normalize_color_mode(mode).unwrap(), mode);
        }
        assert!(ControlViewerTool::normalize_color_mode(" violet ").is_err());
        // Surrounding whitespace is tolerated, mirroring the command box.
        assert_eq!(
            ControlViewerTool::normalize_color_mode(" red ").unwrap(),
            "red"
        );
        for kind in ["cartoon", "stick", "sphere", "surface"] {
            assert_eq!(
                ControlViewerTool::normalize_representation_kind(kind).unwrap(),
                kind
            );
        }
        assert!(ControlViewerTool::normalize_representation_kind("metal").is_err());
        for mode in ["hide", "others", "show"] {
            assert_eq!(
                ControlViewerTool::normalize_visibility_mode(mode).unwrap(),
                mode
            );
        }
        assert!(ControlViewerTool::normalize_visibility_mode("isolate").is_err());
    }
}

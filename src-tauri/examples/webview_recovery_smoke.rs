//! Optional real-WebView smoke: cargo run -p wisp-tauri --example webview_recovery_smoke
//! Uses disposable hidden windows and fake sessions, never the user's database.
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{Manager, WebviewWindow};

#[path = "../src/ui_health.rs"]
#[allow(dead_code)]
mod ui_health;

#[derive(Default)]
struct AppState {
    stopped: Mutex<Vec<String>>,
    boots: Mutex<HashMap<String, usize>>,
}
impl AppState {
    fn active_frame(&self, label: &str) -> Option<String> {
        (label == "main").then(|| "session-a".to_string())
    }
}
mod agent_turn {
    pub async fn stop_agent(
        state: tauri::State<'_, super::AppState>,
        id: Option<String>,
    ) -> Result<(), String> {
        state
            .stopped
            .lock()
            .unwrap()
            .push(id.expect("native stop must be scoped"));
        Ok(())
    }
}

#[tauri::command]
fn smoke_ready(window: WebviewWindow, state: tauri::State<'_, AppState>) {
    *state
        .boots
        .lock()
        .unwrap()
        .entry(window.label().to_string())
        .or_default() += 1;
}

const HTML: &str = r#"<!doctype html><body>Disposable renderer recovery smoke<script>
window.__TAURI_INTERNALS__.invoke('smoke_ready');
setInterval(()=>window.__TAURI_INTERNALS__.invoke('ui_heartbeat'),1000);
</script></body>"#;
struct SmokeAssets;
impl tauri::Assets<tauri::Wry> for SmokeAssets {
    fn get(&self, _: &tauri::utils::assets::AssetKey) -> Option<Cow<'_, [u8]>> {
        Some(Cow::Borrowed(HTML.as_bytes()))
    }
    fn iter(&self) -> Box<tauri::utils::assets::AssetsIter<'_>> {
        Box::new(std::iter::empty())
    }
    fn csp_hashes(
        &self,
        _: &tauri::utils::assets::AssetKey,
    ) -> Box<dyn Iterator<Item = tauri::utils::assets::CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

fn main() {
    let data = std::env::temp_dir().join(format!("wisp-webview-smoke-{}", std::process::id()));
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "science.wisp.webview-recovery-smoke".into();
    context.config_mut().build.dev_url = None;
    context.set_assets(Box::new(SmokeAssets));
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![ui_health::ui_heartbeat, smoke_ready])
        .setup(move |app| {
            for label in ["main", "proj-health"] {
                tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App("index.html".into()))
                    .title("Wisp disposable recovery smoke").visible(false)
                    .data_directory(data.join(label)).build()?;
            }
            let app = app.handle().clone();
            std::thread::spawn(move || {
                let result = run_smoke(&app);
                match result {
                    Ok(()) => { println!("PASS: real WebView reload; native scoped stop during blocked renderer; sibling preserved"); app.exit(0); }
                    Err(error) => { eprintln!("FAIL: {error}"); app.exit(1); }
                }
            });
            Ok(())
        })
        .run(context).expect("run disposable WebView smoke");
}

fn wait_for(check: impl Fn() -> bool, description: &str) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        if check() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("timed out: {description}"))
}

fn run_smoke(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    wait_for(
        || state.boots.lock().unwrap().len() == 2,
        "both WebViews boot",
    )?;
    let main = app.get_webview_window("main").unwrap();
    let sibling = app.get_webview_window("proj-health").unwrap();
    // Bounded busy loop: verifies the escape path without leaving a wedged process.
    main.eval("const end = performance.now() + 15000; while (performance.now() < end) {}")
        .map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_secs(1));
    let started = Instant::now();
    ui_health::stop_window_agent(&main);
    ui_health::stop_window_agent(&sibling); // No active session must not mean stop all.
    wait_for(|| !state.stopped.lock().unwrap().is_empty(), "native stop")?;
    if started.elapsed() > Duration::from_secs(3) {
        return Err("native stop waited on renderer".into());
    }
    if *state.stopped.lock().unwrap() != ["session-a"] {
        return Err("stop leaked to another session".into());
    }
    ui_health::reload_window(&main, "native smoke");
    wait_for(
        || {
            state
                .boots
                .lock()
                .unwrap()
                .get("main")
                .copied()
                .unwrap_or_default()
                >= 2
        },
        "target reload",
    )?;
    if state.boots.lock().unwrap().get("proj-health") != Some(&1) {
        return Err("sibling was reloaded".into());
    }
    Ok(())
}

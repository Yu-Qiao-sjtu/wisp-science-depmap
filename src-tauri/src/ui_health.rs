//! Window-scoped renderer liveness and native recovery. No Run cancellation.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewWindow};
use wisp_dto::UiHealthSnapshot;

const STALE: Duration = Duration::from_secs(60);
const COOLDOWN: Duration = Duration::from_secs(120);

#[derive(Debug)]
struct WindowHealth {
    last_beat: Instant,
    focused: bool,
    last_attempt: Option<Instant>,
    last_log: Option<Instant>,
    snapshot: UiHealthSnapshot,
}

impl WindowHealth {
    fn new(now: Instant) -> Self {
        Self {
            last_beat: now,
            focused: false,
            last_attempt: None,
            last_log: None,
            snapshot: UiHealthSnapshot::default(),
        }
    }

    fn focus(&mut self, focused: bool, now: Instant) {
        // Background timer throttling is not failure. Allow a full grace period
        // on returning to this window, without borrowing any other window's beat.
        if !focused || !self.focused {
            self.last_beat = now;
        }
        self.focused = focused;
    }

    fn reserve_recovery(&mut self, now: Instant) -> bool {
        if !self.focused || now.duration_since(self.last_beat) < STALE {
            return false;
        }
        if self
            .last_attempt
            .is_some_and(|last| now.duration_since(last) < COOLDOWN)
        {
            return false;
        }
        // Rate limit attempts, including failed reloads. Do not disarm when a
        // reload succeeds: a renderer that never boots must remain recoverable.
        self.last_attempt = Some(now);
        true
    }
}

#[derive(Default)]
struct HealthRegistry {
    windows: HashMap<String, WindowHealth>,
    last_focused: Option<String>,
}

fn registry() -> &'static Mutex<HealthRegistry> {
    static REGISTRY: OnceLock<Mutex<HealthRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

pub(crate) fn note_focus(label: &str, focused: bool) {
    if label == "pet" {
        return;
    }
    let now = Instant::now();
    let mut registry = registry().lock().unwrap();
    registry
        .windows
        .entry(label.to_string())
        .or_insert_with(|| WindowHealth::new(now))
        .focus(focused, now);
    if focused {
        registry.last_focused = Some(label.to_string());
    }
}

pub(crate) fn remove_window(label: &str) {
    let mut registry = registry().lock().unwrap();
    registry.windows.remove(label);
    if registry.last_focused.as_deref() == Some(label) {
        registry.last_focused = None;
    }
}

#[tauri::command]
pub(crate) fn ui_heartbeat(window: WebviewWindow, snapshot: Option<UiHealthSnapshot>) {
    if window.label() == "pet" {
        return;
    }
    let now = Instant::now();
    let mut registry = registry().lock().unwrap();
    let health = registry
        .windows
        .entry(window.label().to_string())
        .or_insert_with(|| WindowHealth::new(now));
    health.last_beat = now;
    health.snapshot = snapshot.unwrap_or_default();
    if health
        .last_log
        .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(60))
    {
        health.last_log = Some(now);
        // Numeric summaries only: no prompts, tool arguments, URLs or error text.
        tracing::info!(target: "wisp", window = window.label(), focused = health.focused,
            diagnostics = ?health.snapshot, "webview health");
    }
}

pub(crate) async fn run_watchdog(app: AppHandle) {
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let windows = app.webview_windows();
        registry()
            .lock()
            .unwrap()
            .windows
            .retain(|label, _| windows.contains_key(label));
        for (label, window) in windows {
            if label == "pet" {
                continue;
            }
            let focused = window.is_focused().unwrap_or(false);
            let now = Instant::now();
            let recover = {
                let mut registry = registry().lock().unwrap();
                let health = registry
                    .windows
                    .entry(label.clone())
                    .or_insert_with(|| WindowHealth::new(now));
                health.focus(focused, now);
                health.reserve_recovery(now)
            };
            if recover {
                reload_window(&window, "heartbeat timeout");
            }
        }
    }
}

pub(crate) fn reload_window(window: &WebviewWindow, reason: &str) {
    // Reload only the WebView; AppState, agent runtimes and RunManager stay alive.
    let now = Instant::now();
    {
        let mut registry = registry().lock().unwrap();
        let health = registry
            .windows
            .entry(window.label().to_string())
            .or_insert_with(|| WindowHealth::new(now));
        health.last_attempt = Some(now);
        health.last_beat = now;
    }
    let result = window.reload();
    tracing::warn!(target: "wisp", window = window.label(), reason,
        success = result.is_ok(), error = ?result.err(), "webview recovery requested");
}

pub(crate) fn stop_window_agent(window: &WebviewWindow) {
    let app = window.app_handle().clone();
    let Some(state) = app.try_state::<crate::AppState>() else {
        return;
    };
    // Never pass None to stop_agent: its legacy meaning is all sessions.
    let Some(session_id) = state
        .active_frame(window.label())
        .filter(|id| !id.is_empty())
    else {
        return;
    };
    tracing::info!(target: "wisp", window = window.label(), "native stop requested");
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::agent_turn::stop_agent(app.state(), Some(session_id)).await {
            tracing::warn!(target: "wisp", %error, "native stop failed");
        }
    });
}

#[cfg(target_os = "windows")]
pub(crate) fn last_workspace_window(app: &AppHandle) -> Option<WebviewWindow> {
    let label = registry().lock().unwrap().last_focused.clone();
    label
        .and_then(|label| app.get_webview_window(&label))
        .or_else(|| app.get_webview_window("main"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_window_cannot_mask_another_window_and_cooldowns_are_independent() {
        let start = Instant::now();
        let mut windows = HashMap::from([
            ("main", WindowHealth::new(start)),
            ("project-a", WindowHealth::new(start)),
        ]);
        for health in windows.values_mut() {
            health.focus(true, start);
        }
        windows.get_mut("main").unwrap().last_beat = start + Duration::from_secs(59);
        let now = start + STALE;
        assert!(!windows.get_mut("main").unwrap().reserve_recovery(now));
        assert!(windows.get_mut("project-a").unwrap().reserve_recovery(now));
        assert!(!windows
            .get_mut("project-a")
            .unwrap()
            .reserve_recovery(now + Duration::from_secs(10)));
        assert!(windows
            .get_mut("main")
            .unwrap()
            .reserve_recovery(now + STALE));
    }

    #[test]
    fn silent_startup_and_failed_reloads_remain_recoverable() {
        let now = Instant::now();
        let mut health = WindowHealth::new(now);
        health.focus(true, now);
        assert!(!health.reserve_recovery(now + STALE - Duration::from_secs(1)));
        assert!(health.reserve_recovery(now + STALE));
        assert!(health.reserve_recovery(now + STALE + COOLDOWN));
        health.last_beat = now + STALE + COOLDOWN;
        assert!(!health.reserve_recovery(health.last_beat + Duration::from_secs(5)));
    }

    #[test]
    fn background_and_focus_transitions_get_a_grace_period() {
        let now = Instant::now();
        let later = now + Duration::from_secs(600);
        let mut health = WindowHealth::new(now);
        health.focus(false, now);
        assert!(!health.reserve_recovery(later));
        health.focus(true, later);
        assert!(!health.reserve_recovery(later));
        health.focus(true, later + STALE);
        assert!(health.reserve_recovery(later + STALE));
    }
}

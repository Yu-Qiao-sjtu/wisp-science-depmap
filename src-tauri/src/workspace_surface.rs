//! A primary document and its native window are different objects. In particular,
//! adding an MCP App child makes Tauri's `WebviewWindow` command extractor fail.
//! Keep native window operations and document operations explicitly separated.
use std::{collections::HashMap, ops::Deref};
use tauri::{
    ipc::{CommandArg, CommandItem, InvokeError},
    Manager, Webview, Window,
};

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceSurface {
    window: Window,
    webview: Webview,
}

pub(crate) fn is_primary_document(window_label: &str, webview_label: &str) -> bool {
    window_label == webview_label && !webview_label.starts_with("mcp-app-child-")
}

impl WorkspaceSurface {
    pub(crate) fn from_webview(webview: Webview) -> Result<Self, String> {
        let window = webview.window();
        if !is_primary_document(window.label(), webview.label()) {
            return Err("This command requires the primary workspace WebView.".into());
        }
        Ok(Self { window, webview })
    }

    pub(crate) fn webview(&self) -> &Webview {
        &self.webview
    }
    pub(crate) fn reload(&self) -> tauri::Result<()> {
        // Native recovery cannot depend on the unresponsive primary JS page.
        if let Some(children) = self
            .app_handle()
            .try_state::<crate::mcp_app_children::McpAppChildren>()
        {
            let retired = children
                .registry
                .lock()
                .unwrap()
                .reset_owner(self.label(), false);
            crate::mcp_app_children::retire_native(self.app_handle(), retired.children);
        }
        self.webview.reload()
    }
    pub(crate) fn eval(&self, script: impl Into<String>) -> tauri::Result<()> {
        self.webview.eval(script)
    }
    pub(crate) fn focus_document(&self) -> tauri::Result<()> {
        self.webview.set_focus()
    }
    pub(crate) fn set_focus(&self) -> tauri::Result<()> {
        self.window.set_focus()?;
        self.focus_document()
    }
}

// Native APIs (size, HWND, menu, focus, window close) never select an arbitrary
// child. Explicit methods above are the only operations on the primary page.
impl Deref for WorkspaceSurface {
    type Target = Window;
    fn deref(&self) -> &Self::Target {
        &self.window
    }
}

impl<'de> CommandArg<'de, tauri::Wry> for WorkspaceSurface {
    fn from_command(command: CommandItem<'de, tauri::Wry>) -> Result<Self, InvokeError> {
        Self::from_webview(command.message.webview()).map_err(InvokeError::from)
    }
}

pub(crate) trait WorkspaceManager: Manager<tauri::Wry> {
    fn workspace_surface(&self, label: &str) -> Option<WorkspaceSurface> {
        self.get_webview(label)
            .and_then(|w| WorkspaceSurface::from_webview(w).ok())
    }
    fn workspace_surfaces(&self) -> HashMap<String, WorkspaceSurface> {
        self.webviews()
            .into_iter()
            .filter_map(|(label, w)| {
                WorkspaceSurface::from_webview(w)
                    .ok()
                    .map(|surface| (label, surface))
            })
            .collect()
    }
}
impl<T: Manager<tauri::Wry>> WorkspaceManager for T {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_child_cannot_impersonate_its_owner_even_in_a_multiwebview_window() {
        for label in ["main", "proj-a", "home-b", "terminal-c", "pet"] {
            assert!(is_primary_document(label, label));
            assert!(!is_primary_document(label, "mcp-app-child-123"));
        }
        assert!(!is_primary_document("main", "proj-a"));
        assert!(!is_primary_document(
            "mcp-app-child-123",
            "mcp-app-child-123"
        ));
    }
}

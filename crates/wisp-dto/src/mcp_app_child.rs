//! Internal desktop App-host contract. Not an extension to the MCP wire protocol.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpAppHostInfo {
    pub backend: String,
    pub owner_epoch: String,
}

/// CSS client coordinates plus the primary document's viewport. Native bounds
/// are calculated from the actual WebView size, not a guessed system DPI.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpAppChildBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub visible: bool,
    pub revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpAppChildHandle {
    pub owner_epoch: String,
    pub mount_serial: u64,
    pub child_label: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpAppChildCloseReason {
    Suspend,
    UserClose,
    Replace,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppChildBootstrap {
    pub handle: McpAppChildHandle,
    pub instance_id: String,
    pub payload: Value,
    pub host_context: Value,
    pub version: String,
    pub server_tools_available: bool,
}

/// Both endpoints validate the method allowlist; plugin strings are never
/// concatenated into executable JavaScript when delivering this envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppChildDelivery {
    pub kind: String,
    pub request_id: Option<String>,
    pub method: Option<String>,
    pub params: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppChildRequest {
    pub id: Option<Value>,
    pub method: String,
    pub params: Value,
}

// This trusted shell is a separate native WebView. It has no workspace event
// subscription, no filesystem APIs and no reference to the main document.
import { injectMcpAppCsp, injectMotifWispBridge } from "../mcp_app_protocol.js";

const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
let frame, bootstrap, initialized = false, closed = false;
const pendingActions = new Map();
const post = (message) => frame?.contentWindow?.postMessage(message, "*");
const errorText = (error) => String(error?.message || error).slice(0, 512);
const reply = (message, result, error) => post({jsonrpc:"2.0", id:message.id,
  ...(error ? {error:{code:-32603, message:errorText(error)}} : {result})});

function sendData() {
  post({jsonrpc:"2.0", method:"ui/notifications/tool-input", params:{arguments:bootstrap.payload.arguments || {}}});
  post({jsonrpc:"2.0", method:"ui/notifications/tool-result", params:bootstrap.payload.result || {content:[]}});
  post({jsonrpc:"2.0", method:"ui/notifications/host-context-changed", params:bootstrap.hostContext});
}
async function ready() {
  if (initialized || closed) return;
  initialized = true;
  sendData();
}

window.__wispMcpReceive = (message) => {
  if (message.kind === "teardown") {
    closed = true;
    pendingActions.clear();
    post({jsonrpc:"2.0", id:"wisp-native-teardown", method:"ui/resource-teardown", params:{reason:"view closed; unsubmitted state is not retained"}});
    return;
  }
  if (closed) return;
  if (message.kind === "host-context") {
    bootstrap.hostContext = message.params;
    if (initialized) post({jsonrpc:"2.0", method:"ui/notifications/host-context-changed", params:message.params});
  }
  if (message.kind === "request" && initialized) {
    const {requestId, method, params} = message;
    if (!["wisp/motif-add-records", "wisp/motif-get-selection", "ui/notifications/tool-result"].includes(method)) return;
    if (method === "ui/notifications/tool-result") {
      bootstrap.payload.result = params;
      post({jsonrpc:"2.0", method, params});
      void invoke("mcp_app_child_action_reply", {requestId, result:{}, error:null}).catch(()=>{});
    } else {
      pendingActions.set(requestId, method);
      post({jsonrpc:"2.0", method, params:{...params, requestId}});
      setTimeout(()=>pendingActions.delete(requestId), 5000);
    }
  }
};

window.addEventListener("message", async (event) => {
  if (closed || event.source !== frame?.contentWindow || event.origin !== "null") return;
  const message = event.data;
  if (!message || message.jsonrpc !== "2.0" || typeof message.method !== "string") return;
  if (message.method === "wisp/notifications/motif-bridge-ready") {
    if (bootstrap.payload?.tool?.name === "motif_open_workbench") await ready();
    return;
  }
  if (message.method.startsWith("wisp/notifications/motif-")) {
    const requestId = message.params?.requestId;
    const expected = pendingActions.get(requestId);
    const method = message.method;
    if (!expected || !["wisp/notifications/motif-bridge-error",
      expected === "wisp/motif-get-selection" ? "wisp/notifications/motif-selection" : "wisp/notifications/motif-records-added"].includes(method)) return;
    pendingActions.delete(requestId);
    await invoke("mcp_app_child_action_reply", {requestId,
      result:message.params || {}, error:method.endsWith("bridge-error") ? errorText(message.params?.message) : null}).catch(()=>{});
    return;
  }
  try {
    const response = await invoke("mcp_app_child_request", {request:{id:message.id ?? null, method:message.method, params:message.params ?? {}}});
    if (closed) return;
    if (message.id != null) post(response);
    if (message.method === "ui/notifications/initialized") await ready();
  } catch (error) {
    if (!closed && message.id != null) reply(message, null, error);
  }
});

try {
  bootstrap = await invoke("mcp_app_child_bootstrap");
  frame = document.createElement("iframe");
  frame.title = bootstrap.payload?.tool?.title || bootstrap.payload?.tool?.name || "MCP App";
  frame.setAttribute("sandbox", "allow-scripts");
  frame.setAttribute("referrerpolicy", "no-referrer");
  let html = bootstrap.payload.resource.text;
  if (bootstrap.payload?.tool?.name === "motif_open_workbench") html = injectMotifWispBridge(html);
  // Respect a guest's own Escape handler first, then forward to the owner's
  // existing window-level Escape stack. Events cannot bubble across WebViews.
  html += `<script>addEventListener('keydown',e=>{if(e.key==='Escape'&&!e.isComposing)queueMicrotask(()=>{if(!e.defaultPrevented)parent.postMessage({jsonrpc:'2.0',method:'wisp/escape',params:{}},'*')})});<\/script>`;
  frame.srcdoc = injectMcpAppCsp(html, bootstrap.payload?.resource?._meta);
  document.body.replaceChildren(frame);
  if (bootstrap.serverToolsAvailable === false) {
    const status = document.createElement("div");
    status.id = "connection-status"; status.setAttribute("role", "status");
    status.textContent = String(bootstrap.hostContext?.locale || "").startsWith("zh")
      ? "原 MCP 连接不可用：仅显示保存的结果；不会自动重执行工具。"
      : "Original MCP connection unavailable: saved results only. Tools are not automatically replayed.";
    document.body.prepend(status);
  }
  // Show the host after the shell is ready, not after guest JS runs: a guest
  // that blocks on startup must still be visible/closable via the parent.
  await invoke("mcp_app_child_ready");
} catch (error) {
  const alert = document.createElement("div");
  alert.id = "error";
  alert.textContent = `MCP App unavailable: ${errorText(error)}. Close or retry from Wisp.`;
  document.body.replaceChildren(alert);
}

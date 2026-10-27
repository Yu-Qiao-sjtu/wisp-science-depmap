// Wisp DepMap Browser Bridge protocol constants (Protocol v2).
var WISP_PROTOCOL = {
  version: 2,
  extensionVersion: "0.5.0",
  product: "wisp-depmap",
  capabilities: [
    "article_scan",
    "asset_download",
    "selector_screenshot",
    "full_page_screenshot",
    "managed_tabs",
    "conditional_wait",
    "session",
    "pause_control",
    "runtime_reload",
    "chatgpt_turn",
    "chat_turn",
    "side_panel",
    "tab_scope"
  ]
};

function readBridgeConfig() {
  var session = "shared";
  var endpoint = "ws://127.0.0.1:28765";
  var product = WISP_PROTOCOL.product || "wisp-depmap";
  try {
    if (typeof WISP_BRIDGE_CONFIG === "object" && WISP_BRIDGE_CONFIG) {
      if (WISP_BRIDGE_CONFIG.session) session = String(WISP_BRIDGE_CONFIG.session);
      if (WISP_BRIDGE_CONFIG.endpoint) endpoint = String(WISP_BRIDGE_CONFIG.endpoint);
    }
  } catch (_) {}
  return { session: session, endpoint: endpoint, product: product };
}

// browserInstance identifies one browser process: tab and window ids are only
// meaningful inside it, so Wisp drops remembered ids when it changes.
function handshakePayload(tabs, paused, browserInstance) {
  var cfg = readBridgeConfig();
  var payload = {
    type: "ext_ready",
    product: cfg.product,
    protocol_version: WISP_PROTOCOL.version,
    extension_version: WISP_PROTOCOL.extensionVersion,
    capabilities: WISP_PROTOCOL.capabilities,
    session: cfg.session,
    endpoint: cfg.endpoint,
    paused: !!paused,
    tabs: tabs || []
  };
  if (browserInstance) payload.browser_instance = String(browserInstance);
  return payload;
}

function parseIncomingRequest(request) {
  if (!request || typeof request !== "object") return { kind: "invalid" };
  if (request.op) {
    return {
      kind: "v2",
      id: request.id,
      op: request.op,
      tabId: request.tabId,
      timeoutMs: request.timeoutMs,
      payload: request.payload || {},
      wait: request.wait || null
    };
  }
  if (request.id && request.code !== undefined) {
    return {
      kind: "v1",
      id: request.id,
      code: request.code,
      tabId: request.tabId,
      timeoutMs: request.timeoutMs
    };
  }
  return { kind: "invalid", id: request.id };
}

if (typeof self !== "undefined") {
  self.WISP_PROTOCOL = WISP_PROTOCOL;
  self.readBridgeConfig = readBridgeConfig;
  self.handshakePayload = handshakePayload;
  self.parseIncomingRequest = parseIncomingRequest;
}

// Guest document policy shared by the legacy host and the isolated shell.
const escAttr = (s) => String(s).replaceAll("&", "&amp;").replaceAll("\"", "&quot;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");

export function injectMcpAppCsp(html, resourceMeta) {
  const csp = resourceMeta?.ui?.csp || resourceMeta?.csp || {};
  const safeOrigins = (values, websocket = false) => (Array.isArray(values) ? values : [])
    .filter((value) => typeof value === "string"
      && new RegExp(`^(?:https${websocket ? "|wss" : ""}):\\/\\/(?:\\*\\.)?[a-z0-9.-]+(?::\\d+)?$`, "i").test(value));
  const connect = safeOrigins(csp.connectDomains, true);
  const resources = safeOrigins(csp.resourceDomains);
  const frames = safeOrigins(csp.frameDomains);
  const bases = safeOrigins(csp.baseUriDomains);
  const policy = [
    "default-src 'none'",
    `script-src 'unsafe-inline' 'unsafe-eval' blob: ${resources.join(" ")}`.trim(),
    `style-src 'unsafe-inline' ${resources.join(" ")}`.trim(),
    `img-src data: blob: ${resources.join(" ")}`.trim(),
    `font-src data: ${resources.join(" ")}`.trim(),
    `media-src blob: ${resources.length ? resources.join(" ") : "'none'"}`,
    `connect-src ${connect.length ? connect.join(" ") : "'none'"}`,
    `frame-src ${frames.length ? frames.join(" ") : "'none'"}`,
    `base-uri ${bases.length ? bases.join(" ") : "'self'"}`,
    "object-src 'none'",
    "form-action 'none'",
  ].join("; ");
  const tag = `<meta http-equiv="Content-Security-Policy" content="${escAttr(policy)}">`;
  if (/<head(\s[^>]*)?>/i.test(html)) {
    return html.replace(/<head(\s[^>]*)?>/i, (head) => `${head}${tag}`);
  }
  return `<!doctype html><html><head>${tag}</head><body>${html}</body></html>`;
}

export function injectMotifWispBridge(html) {
  const script = `<script>
(() => {
  const reply = (method, params) => parent.postMessage({ jsonrpc: "2.0", method, params }, "*");
  const activeRecord = () => {
    try { return typeof window.motifGetActiveRecord === "function" ? window.motifGetActiveRecord() : null; }
    catch { return null; }
  };
  const recordSequence = (record) => typeof record?.seq === "string"
    ? record.seq.toUpperCase()
    : typeof record?.sequence === "string"
      ? record.sequence.toUpperCase()
      : "";
  const coordinateNumber = (value) => Number(String(value || "").replace(/[^0-9]/g, ""));
  const sequenceRange = (record, start, end, wrap = false) => {
    const source = recordSequence(record);
    if (!start || !end || start > source.length || end > source.length) return "";
    return wrap || start > end
      ? source.slice(start - 1) + source.slice(0, end)
      : source.slice(start - 1, end);
  };
  const renderedSelection = (record) => {
    const label = document.querySelector(
      ".motif-cs-selection-bar:not([data-empty='true']) .motif-cs-selection-name",
    )?.textContent?.trim() || "";
    const match = label.match(/^([0-9][0-9,. ]*)-([0-9][0-9,. ]*)( wrap)? \\(([0-9][0-9,. ]*)\\)$/);
    if (!match) return null;
    const start = coordinateNumber(match[1]);
    const end = coordinateNumber(match[2]);
    const length = coordinateNumber(match[4]);
    const sequence = sequenceRange(record, start, end, Boolean(match[3]));
    return sequence.length === length ? { start, end, strand: "forward", sequence } : null;
  };
  const featureSelection = (record) => {
    const label = document.querySelector(
      ".motif-cs-selection-bar:not([data-empty='true']) .motif-cs-selection-name",
    )?.textContent?.trim() || "";
    const labelMatch = label.match(/^(.*?)\s+([0-9][0-9,. ]*)-([0-9][0-9,. ]*)(?:\s+wrap)?$/);
    const annotations = Array.isArray(record?.annotations)
      ? record.annotations
      : Array.isArray(record?.features) ? record.features : [];
    const selectedNode = document.querySelector(
      ".motif-pm-feature[aria-pressed='true'][data-feature-id], .motif-cs-feature-block[aria-pressed='true']",
    );
    const selectedId = selectedNode?.getAttribute("data-feature-id") || "";
    let feature = selectedId
      ? annotations.find((annotation) => String(annotation?.id || "") === selectedId)
      : null;
    if (!feature && labelMatch) {
      const start = coordinateNumber(labelMatch[2]);
      const end = coordinateNumber(labelMatch[3]);
      const name = labelMatch[1].trim();
      feature = annotations.find((annotation) => (
        Number(annotation?.start) + 1 === start
        && Number(annotation?.end) === end
        && String(annotation?.name || "").trim() === name
      ));
    }
    if (!feature) return null;
    const start = Number(feature.start) + 1;
    const end = Number(feature.end);
    const sequence = sequenceRange(record, start, end);
    if (!sequence) return null;
    return {
      start,
      end,
      strand: Number(feature.strand) === -1 ? "reverse" : "forward",
      sequence,
      featureName: String(feature.name || "").trim() || undefined,
    };
  };
  const nativeSelection = (record) => {
    const source = recordSequence(record);
    const raw = String(getSelection()?.toString() || "");
    const sequence = raw.replace(/[^A-Za-z*.-]/g, "").toUpperCase();
    if (!source || !sequence) return null;
    const offset = source.indexOf(sequence);
    return offset < 0 ? null : { start: offset + 1, end: offset + sequence.length, strand: "forward", sequence };
  };
  let lastNativeSelection = null;
  let selectionLengthFrame = 0;
  const updateSelectionLength = () => {
    selectionLengthFrame = 0;
    const bar = document.querySelector(".motif-cs-selection-bar");
    if (!bar) return;
    const record = activeRecord();
    const recordId = String(record?.id || "");
    const selection = renderedSelection(record)
      || featureSelection(record)
      || (bar.matches(":not([data-empty='true'])")
        ? null
        : nativeSelection(record)
          || (lastNativeSelection?.recordId === recordId ? lastNativeSelection : null));
    let badge = bar.querySelector("[data-wisp-motif-selection-length]");
    if (!selection?.sequence) {
      badge?.remove();
      return;
    }
    const length = Array.from(selection.sequence).length;
    const label = length.toLocaleString() + " bp";
    if (!badge) {
      badge = document.createElement("span");
      badge.setAttribute("data-wisp-motif-selection-length", "");
      badge.style.cssText = "margin-left:auto;padding-left:10px;white-space:nowrap;font-weight:700;font-variant-numeric:tabular-nums;color:currentColor;pointer-events:none";
      bar.appendChild(badge);
    }
    if (badge.textContent !== label) badge.textContent = label;
    badge.setAttribute("aria-label", "Selected sequence length: " + label);
  };
  const scheduleSelectionLengthUpdate = () => {
    if (selectionLengthFrame) return;
    selectionLengthFrame = requestAnimationFrame(updateSelectionLength);
  };
  const rememberNativeSelection = () => {
    const record = activeRecord();
    const selection = nativeSelection(record);
    if (selection) lastNativeSelection = { recordId: String(record?.id || ""), ...selection };
    scheduleSelectionLengthUpdate();
  };
  document.addEventListener("selectionchange", rememberNativeSelection);
  document.addEventListener("pointerup", rememberNativeSelection, true);
  document.addEventListener("keyup", rememberNativeSelection, true);
  const scrollSelectedFeatureIntoView = () => {
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const block = document.querySelector(".motif-cs-feature-block[aria-pressed='true']");
      const pane = block?.closest(".motif-cs-sequence-column");
      if (!block || !pane) return;
      const blockRect = block.getBoundingClientRect();
      const paneRect = pane.getBoundingClientRect();
      pane.scrollTop = Math.max(0, pane.scrollTop + blockRect.top - paneRect.top
        - Math.max(0, (pane.clientHeight - blockRect.height) / 2));
    }));
  };
  const scheduleFeatureFocus = (target) => {
    if (!(target instanceof Element) || !target.closest(".motif-pm-feature[data-feature-id]")) return;
    lastNativeSelection = null;
    scrollSelectedFeatureIntoView();
  };
  document.addEventListener("click", (event) => scheduleFeatureFocus(event.target), true);
  document.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") scheduleFeatureFocus(event.target);
  }, true);
  const featureFocusObserver = new MutationObserver(() => {
    if (document.querySelector(".motif-cs-feature-block[aria-pressed='true']")) {
      scrollSelectedFeatureIntoView();
    }
    scheduleSelectionLengthUpdate();
  });
  featureFocusObserver.observe(document.body, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ["aria-pressed"],
  });
  scheduleSelectionLengthUpdate();
  addEventListener("message", (event) => {
    if (event.source !== parent) return;
    const message = event.data || {};
    if (message.jsonrpc !== "2.0") return;
    if (message.method === "wisp/motif-add-records") {
      try {
        if (typeof window.motifAddRecords !== "function") throw new Error("Motif record API is not ready.");
        const records = Array.isArray(message.params?.records) ? message.params.records : [];
        window.motifAddRecords(records);
        reply("wisp/notifications/motif-records-added", { requestId: message.params?.requestId, count: records.length });
      } catch (error) {
        reply("wisp/notifications/motif-bridge-error", { requestId: message.params?.requestId, message: error instanceof Error ? error.message : String(error) });
      }
    }
    if (message.method === "wisp/motif-get-selection") {
      try {
        const record = activeRecord();
        const recordId = String(record?.id || "");
        const selection = renderedSelection(record)
          || featureSelection(record)
          || (document.querySelector(".motif-cs-selection-bar:not([data-empty='true'])")
            ? null
            : nativeSelection(record)
              || (lastNativeSelection?.recordId === recordId ? lastNativeSelection : null));
        if (!record || !selection) throw new Error("Select a sequence range in Motif first.");
        reply("wisp/notifications/motif-selection", {
          requestId: message.params?.requestId,
          recordName: String(record.name || record.id || "Motif record"),
          recordId,
          molecule: String(record.type || record.molecule || "dna"),
          start: selection.start,
          end: selection.end,
          strand: selection.strand || "forward",
          sequence: selection.sequence,
          featureName: selection.featureName,
        });
      } catch (error) {
        reply("wisp/notifications/motif-bridge-error", { requestId: message.params?.requestId, message: error instanceof Error ? error.message : String(error) });
      }
    }
  });
  let readyAttempts = 0;
  const announceReady = () => {
    if (typeof window.motifGetActiveRecord === "function") {
      reply("wisp/notifications/motif-bridge-ready", {});
      return;
    }
    readyAttempts += 1;
    if (readyAttempts < 200) setTimeout(announceReady, 50);
  };
  announceReady();
})();
</script>`;
  // Motif bundles may contain literal `</body>` text inside minified scripts.
  // Never splice the document with a regex: HTML parsers accept a trailing
  // script after </html> and place it in the document body without corrupting
  // any of Motif's original script boundaries.
  return `${html}${script}`;
}


// Camera math and PyMOL highlight for the Mol* viewer window.
// Kept free of the DOM so the transforms can be tested without a GPU.

// Chrome strings for the viewer window, English and Chinese. Mol*'s bundled
// panels keep Mol*'s own language; this table covers only the toolbar and
// status line the desktop shell owns. The desktop UI language picks the
// column, delivered via the viewer URL and the viewer://locale event.
const CHROME_I18N = {
  en: {
    "title.structure": "Structure viewer",
    "title.trajectory": "Trajectory viewer",
    "button.select": "Select",
    "button.zoom": "Zoom",
    "button.label": "Label",
    "button.distance": "Distance",
    "button.color": "Color",
    "button.apply": "Apply",
    "button.export": "Export PNG",
    "button.clear": "Clear",
    "button.explain": "Explain in chat",
    "color.element": "Element",
    "color.chain": "Chain",
    "color.spectrum": "Spectrum",
    "color.red": "Red",
    "color.green": "Green",
    "color.blue": "Blue",
    "color.yellow": "Yellow",
    "color.cyan": "Cyan",
    "color.magenta": "Magenta",
    "color.orange": "Orange",
    "color.white": "White",
    "color.gray": "Gray",
    "color.clearPaint": "Clear paint",
    "visibility.hide": "Hide selection",
    "visibility.others": "Hide others",
    "visibility.show": "Show all",
    "hint.color": "Color by element, chain, spectrum, or a solid color",
    "hint.visibility": "Hide the selection, hide everything else, or show everything",
    "hint.splitter": "Drag to resize the panel",
    "status.initializing": "initializing viewer…",
    "status.downloading": "downloading {id}…",
    "status.loading": "loading {name}…",
    "status.loadingTrajectory": "loading {topo} + {traj}…",
    "status.camera": "camera {action}",
    "status.zoomedSelection": "zoomed to selection",
    "status.zoomedStructure": "zoomed to whole structure",
    "status.labelsOn": "labels on {count} selection{s}",
    "status.labelsOff": "labels off",
    "status.distance": "distance {value} Å",
    "status.distancesCleared": "distances cleared",
    "status.coloredBy": "colored by {what}",
    "status.selectionHidden": "selection hidden",
    "status.othersHidden": "everything except the selection hidden",
    "status.everythingShown": "everything shown",
    "status.selectionCleared": "selection cleared",
    "status.savingImage": "saving image…",
    "status.saved": "saved {name}",
    "status.exportCancelled": "export cancelled",
    "status.noCanvas": "no canvas to export yet",
    "status.explainNeedsStructure": "load a structure before asking for an explanation",
    "status.askingChat": "asking chat…",
    "status.explanationRequested": "explanation requested in chat",
    "cmd.selected": "selected {expression}",
    "cmd.shown": "{representation} shown",
    "cmd.hidden": "{representation} hidden",
    "cmd.coloredByTheme": "colored by {theme}",
    "cmd.coloredSolid": "colored {color}",
    "cmd.zoomedSelection": "zoomed to selection",
    "cmd.zoomedStructure": "zoomed to whole structure",
    "cmd.zoomedExpression": "zoomed to {expression}",
    "cmd.centeredSelection": "centered on selection",
    "cmd.centeredStructure": "centered on structure",
    "error.emptyCommand": "type a command first",
    "error.outsideSubset": "'{verb}' is outside the PyMOL subset (select, show, hide, color, zoom, center)",
    "error.needsRepresentation": "{command} needs a representation: cartoon, sticks, spheres, or surface",
    "error.unsupportedRepresentation": "unsupported representation {name}; expected cartoon, sticks, spheres, or surface",
    "error.needsColor": "color needs a theme or color name",
    "error.unsupportedColor": "unsupported color {name}; expected element, chain, spectrum, or a PyMOL color name",
    "error.needsExpression": "select needs a selection expression",
    "error.noAtomsMatched": "no atoms matched {expression}",
    "error.loadBeforeColoring": "load a structure before coloring it",
    "error.loadBeforeHiding": "load a structure before hiding parts of it",
    "error.loadBeforeCentering": "load a structure before centering it",
    "error.pickTwoAtoms": "pick two atoms before measuring",
    "error.selectBeforeLabeling": "select residues before labeling them",
  },
  zh: {
    "title.structure": "结构查看器",
    "title.trajectory": "轨迹查看器",
    "button.select": "选择",
    "button.zoom": "缩放",
    "button.label": "标签",
    "button.distance": "距离",
    "button.color": "着色",
    "button.apply": "应用",
    "button.export": "导出 PNG",
    "button.clear": "清除",
    "button.explain": "在对话中解释",
    "color.element": "元素",
    "color.chain": "链",
    "color.spectrum": "光谱",
    "color.red": "红色",
    "color.green": "绿色",
    "color.blue": "蓝色",
    "color.yellow": "黄色",
    "color.cyan": "青色",
    "color.magenta": "品红",
    "color.orange": "橙色",
    "color.white": "白色",
    "color.gray": "灰色",
    "color.clearPaint": "清除着色",
    "visibility.hide": "隐藏所选",
    "visibility.others": "隐藏其它",
    "visibility.show": "全部显示",
    "hint.color": "按元素、链、光谱或纯色着色",
    "hint.visibility": "隐藏所选、隐藏其它或全部显示",
    "hint.splitter": "拖动调整面板宽度",
    "status.initializing": "正在初始化查看器…",
    "status.downloading": "正在下载 {id}…",
    "status.loading": "正在加载 {name}…",
    "status.loadingTrajectory": "正在加载 {topo} + {traj}…",
    "status.camera": "相机 {action}",
    "status.zoomedSelection": "已缩放到所选",
    "status.zoomedStructure": "已缩放到整个结构",
    "status.labelsOn": "已为 {count} 个所选加标签",
    "status.labelsOff": "已关闭标签",
    "status.distance": "距离 {value} Å",
    "status.distancesCleared": "已清除距离",
    "status.coloredBy": "已按{what}着色",
    "status.selectionHidden": "已隐藏所选",
    "status.othersHidden": "已隐藏所选之外的全部",
    "status.everythingShown": "已显示全部",
    "status.selectionCleared": "已清除选择",
    "status.savingImage": "正在保存图像…",
    "status.saved": "已保存 {name}",
    "status.exportCancelled": "已取消导出",
    "status.noCanvas": "暂无可导出的画布",
    "status.explainNeedsStructure": "请先加载结构再请求解释",
    "status.askingChat": "正在询问对话…",
    "status.explanationRequested": "已在对话中请求解释",
    "cmd.selected": "已选择 {expression}",
    "cmd.shown": "已显示 {representation}",
    "cmd.hidden": "已隐藏 {representation}",
    "cmd.coloredByTheme": "已按{theme}着色",
    "cmd.coloredSolid": "已着色 {color}",
    "cmd.zoomedSelection": "已缩放到所选",
    "cmd.zoomedStructure": "已缩放到整个结构",
    "cmd.zoomedExpression": "已缩放到 {expression}",
    "cmd.centeredSelection": "已居中到所选",
    "cmd.centeredStructure": "已居中到结构",
    "error.emptyCommand": "请先输入命令",
    "error.outsideSubset": "“{verb}”不在 PyMOL 子集内（select、show、hide、color、zoom、center）",
    "error.needsRepresentation": "{command} 需要表示方式：cartoon、sticks、spheres 或 surface",
    "error.unsupportedRepresentation": "不支持的表示方式 {name}；应为 cartoon、sticks、spheres 或 surface",
    "error.needsColor": "color 需要主题或颜色名",
    "error.unsupportedColor": "不支持的颜色 {name}；应为 element、chain、spectrum 或 PyMOL 颜色名",
    "error.needsExpression": "select 需要选择表达式",
    "error.noAtomsMatched": "没有原子匹配 {expression}",
    "error.loadBeforeColoring": "请先加载结构再着色",
    "error.loadBeforeHiding": "请先加载结构再隐藏",
    "error.loadBeforeCentering": "请先加载结构再居中",
    "error.pickTwoAtoms": "请先拾取两个原子再测量",
    "error.selectBeforeLabeling": "请先选择残基再加标签",
  },
};

let chromeLanguage = "en";

function normalizeChromeLocale(value) {
  const raw = String(value || "").trim().toLowerCase();
  return raw === "zh" || raw === "zh-cn" || raw === "zh-tw" ? "zh" : "en";
}

function setChromeLocale(value) {
  chromeLanguage = normalizeChromeLocale(value);
  return chromeLanguage;
}

// Translate one chrome key with `{placeholder}` interpolation. Unknown keys
// return the key itself so missing strings stay visible instead of blank.
function chromeText(locale, key, params) {
  const table = CHROME_I18N[normalizeChromeLocale(locale)] || CHROME_I18N.en;
  let text = table[key] !== undefined ? table[key] : CHROME_I18N.en[key];
  if (text === undefined) return key;
  if (params) {
    text = String(text).replace(/\{(\w+)\}/g, (match, name) => (
      params[name] !== undefined ? String(params[name]) : match
    ));
  }
  return text;
}

function ct(key, params) {
  return chromeText(chromeLanguage, key, params);
}

function cloneSnapshot(snapshot) {
  return {
    ...snapshot,
    position: snapshot.position.slice(),
    target: snapshot.target.slice(),
    up: snapshot.up.slice(),
  };
}

function sub(a, b) {
  return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
}

function add(a, b) {
  return [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
}

function scale(a, k) {
  return [a[0] * k, a[1] * k, a[2] * k];
}

function dot(a, b) {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

function cross(a, b) {
  return [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ];
}

function rodrigues(vector, axis, cos, sin) {
  const along = scale(axis, dot(vector, axis));
  return add(add(scale(vector, cos), scale(cross(axis, vector), sin)), scale(along, 1 - cos));
}

const AXES = { x: [1, 0, 0], y: [0, 1, 0], z: [0, 0, 1] };

// factor > 1 moves the camera closer to the target.
function zoomSnapshot(snapshot, factor) {
  if (!(factor > 0) || !Number.isFinite(factor)) {
    throw new Error("zoom factor must be a positive finite number");
  }
  const next = cloneSnapshot(snapshot);
  const offset = sub(next.position, next.target);
  next.position = add(next.target, scale(offset, 1 / factor));
  if (typeof next.radius === "number") next.radius = next.radius / factor;
  return next;
}

function rotateSnapshot(snapshot, axis, degrees) {
  const axisVector = AXES[axis];
  if (!axisVector) throw new Error("rotate axis must be x, y, or z");
  if (!Number.isFinite(degrees)) throw new Error("rotate degrees must be finite");
  const radians = degrees * Math.PI / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  const next = cloneSnapshot(snapshot);
  const offset = sub(next.position, next.target);
  next.position = add(next.target, rodrigues(offset, axisVector, cos, sin));
  next.up = rodrigues(next.up, axisVector, cos, sin);
  return next;
}

const REPRESENTATIONS = {
  cartoon: "cartoon",
  stick: "ball-and-stick",
  sphere: "spacefill",
  surface: "molecular-surface",
};

function representationTypeName(repr) {
  const cell = repr && repr.cell;
  const params = cell && ((cell.params && cell.params.values) || (cell.transform && cell.transform.params));
  const type = params && params.type;
  if (!type) return "";
  return typeof type === "string" ? type : type.name || "";
}

function structureComponents(plugin) {
  const structures = plugin
    && plugin.managers
    && plugin.managers.structure
    && plugin.managers.structure.hierarchy
    && plugin.managers.structure.hierarchy.current
    && plugin.managers.structure.hierarchy.current.structures
    || [];
  const components = [];
  for (const structure of structures) {
    for (const component of structure.components || []) components.push(component);
  }
  return components;
}

// Turn one Mol* representation on or off for every component currently loaded.
// `kind` is cartoon, stick, sphere, or surface.
async function setStructureRepresentation(plugin, kind, visible) {
  const type = REPRESENTATIONS[kind];
  if (!type) throw new Error("unsupported representation " + kind);
  const components = structureComponents(plugin);
  const matching = [];
  const missing = [];
  for (const component of components) {
    const found = (component.representations || []).some((repr) => representationTypeName(repr) === type);
    if (found) {
      for (const repr of component.representations || []) {
        if (representationTypeName(repr) === type) matching.push(repr);
      }
    } else {
      missing.push(component);
    }
  }
  if (visible) {
    const add = plugin.managers.structure.component
      && plugin.managers.structure.component.addRepresentation;
    if (missing.length && typeof add === "function") await add.call(plugin.managers.structure.component, missing, type);
    return missing.length;
  }
  const remove = plugin.managers.structure.hierarchy.remove;
  if (matching.length && typeof remove === "function") await remove.call(plugin.managers.structure.hierarchy, matching, true);
  return matching.length;
}

// Drop structures already in this Mol* session, then load the new file into
// that same session. A later frame or a newly opened file replaces what is
// on screen instead of leaving a second molecule beside it.
async function clearLoadedStructures(viewer) {
  const plugin = viewer.plugin;
  const hierarchy = plugin
    && plugin.managers
    && plugin.managers.structure
    && plugin.managers.structure.hierarchy;
  if (!hierarchy || typeof hierarchy.remove !== "function") return;
  const structures = (hierarchy.current && hierarchy.current.structures) || [];
  if (structures.length) await hierarchy.remove(structures, true);
}

async function replaceLoadedStructure(viewer, data, format) {
  await clearLoadedStructures(viewer);
  return viewer.loadStructureFromData(data, format);
}

function clearStructureSelection(plugin) {
  const interactivity = plugin && plugin.managers && plugin.managers.interactivity;
  if (!interactivity) return false;
  if (interactivity.lociSelects && typeof interactivity.lociSelects.deselectAll === "function") {
    interactivity.lociSelects.deselectAll();
  }
  if (interactivity.lociHighlights && typeof interactivity.lociHighlights.clearHighlights === "function") {
    interactivity.lociHighlights.clearHighlights();
  }
  return true;
}

// Loci the user currently has selected, one per loaded structure. Residue
// picks, the Select action, and selections delivered with a load all land in
// the same Mol* selection manager.
function currentSelectionLoci(molstar, plugin) {
  const Loci = molstar.lib.loci.Loci;
  const selection = plugin.managers.structure && plugin.managers.structure.selection;
  if (!selection || typeof selection.getLoci !== "function") return [];
  const lociList = [];
  for (const structure of structuresOf(plugin)) {
    const loci = selection.getLoci(structure);
    if (loci && typeof Loci.isEmpty === "function" && !Loci.isEmpty(loci)) lociList.push(loci);
  }
  return lociList;
}

// PyMOL `zoom`/`center` on the current selection: frame the selected loci,
// or the whole structure when nothing is selected. Returns what was framed.
function focusCurrentSelection(molstar, plugin) {
  const lociList = currentSelectionLoci(molstar, plugin);
  if (lociList.length && plugin.managers.camera && typeof plugin.managers.camera.focusLoci === "function") {
    plugin.managers.camera.focusLoci(lociList, { extraRadius: 2, durationMs: 300 });
    return "selection";
  }
  applyCameraPayload(molstar, plugin, { action: "reset" });
  return "structure";
}

// Tag stamped on the label cells this window adds, so hiding labels removes
// only ours and leaves Mol*'s own measurements alone.
const VIEWER_LABEL_TAG = "wisp-viewer-label";

// Tag stamped on distance cells added by the two-atom measurement action.
const VIEWER_DISTANCE_TAG = "wisp-viewer-distance";

// Remove every state cell carrying `tag` (selection + representation pairs
// the tagged measurement created). Returns how many cells were removed.
async function removeTaggedCells(plugin, tag) {
  const state = plugin.state.data;
  const build = state.build();
  let removed = 0;
  for (const [ref, cell] of state.cells) {
    const tags = cell.transform && cell.transform.tags;
    if (Array.isArray(tags) && tags.includes(tag)) {
      build.delete(ref);
      removed += 1;
    }
  }
  if (removed) await build.commit();
  return removed;
}

// PyMOL `label`: residue name+number labels on the current selection. Mol*'s
// label representation derives that text from each selected loci.
async function setSelectionLabels(molstar, plugin, visible) {
  const measurement = plugin.managers.structure && plugin.managers.structure.measurement;
  if (!measurement || typeof measurement.addLabel !== "function") {
    throw new Error("Mol* build is missing selection labels");
  }
  if (!visible) {
    return removeTaggedCells(plugin, VIEWER_LABEL_TAG);
  }
  const lociList = currentSelectionLoci(molstar, plugin);
  if (!lociList.length) {
    throw new Error(ct("error.selectBeforeLabeling"));
  }
  let added = 0;
  for (const loci of lociList) {
    await measurement.addLabel(loci, {
      selectionTags: [VIEWER_LABEL_TAG],
      reprTags: [VIEWER_LABEL_TAG],
    });
    added += 1;
  }
  return added;
}

function structuresOf(plugin) {
  const current = plugin.managers.structure.hierarchy.current.structures || [];
  return current
    .map((entry) => entry.cell && entry.cell.obj && entry.cell.obj.data)
    .filter(Boolean);
}

// Compile a PyMOL expression with the vendored Mol* parser and select those
// atoms. Returns how many structures contributed a non-empty loci.
function highlightPymol(molstar, plugin, expression) {
  if (typeof molstar.scriptToQuery !== "function") {
    throw new Error("Mol* build is missing scriptToQuery");
  }
  const query = molstar.scriptToQuery({ language: "pymol", expression });
  const StructureSelection = molstar.lib.structure.StructureSelection;
  const QueryContext = molstar.lib.structure.QueryContext;
  const selects = plugin.managers.interactivity.lociSelects;
  const highlights = plugin.managers.interactivity.lociHighlights;
  selects.deselectAll();
  highlights.clearHighlights();
  const lociList = [];
  for (const structure of structuresOf(plugin)) {
    const selection = query(new QueryContext(structure));
    if (StructureSelection.isEmpty(selection)) continue;
    const loci = StructureSelection.toLociWithSourceUnits(selection);
    selects.select({ loci }, false);
    highlights.highlight({ loci }, false);
    lociList.push(loci);
  }
  if (lociList.length && plugin.managers.camera.focusLoci) {
    plugin.managers.camera.focusLoci(lociList, { extraRadius: 2, durationMs: 300 });
  }
  return lociList.length;
}

function applyCameraPayload(molstar, plugin, payload) {
  const canvas = plugin.canvas3d;
  if (!canvas || !canvas.camera) throw new Error("viewer camera is not ready");
  const camera = plugin.managers.camera;
  if (payload.action === "reset") {
    const sphere = canvas.boundingSphereVisible;
    const snapshot = canvas.camera.getInvariantFocus(
      sphere.center,
      sphere.radius,
      [0, 1, 0],
      [0, 0, -1],
    );
    camera.setSnapshot(snapshot, 400);
    return snapshot;
  }
  if (payload.action === "zoom") {
    const next = zoomSnapshot(canvas.camera.getSnapshot(), payload.factor);
    camera.setSnapshot(next, 400);
    return next;
  }
  if (payload.action === "rotate") {
    const next = rotateSnapshot(canvas.camera.getSnapshot(), payload.axis, payload.degrees);
    camera.setSnapshot(next, 400);
    return next;
  }
  if (payload.action === "center") {
    const expression = payload.residue ? "resi " + payload.residue : "polymer.protein";
    highlightPymol(molstar, plugin, expression);
    return canvas.camera.getSnapshot();
  }
  throw new Error("unsupported camera action " + payload.action);
}

// PyMOL `dist`: draw and report the distance between the two most recently
// picked atoms. The picked loci come from Mol*'s selection history, the same
// source its own measurement panel uses.
async function measurePickedDistance(molstar, plugin) {
  const selection = plugin.managers.structure && plugin.managers.structure.selection;
  const history = (selection && selection.additionsHistory) || [];
  if (history.length < 2) {
    throw new Error(ct("error.pickTwoAtoms"));
  }
  const [a, b] = history.slice(0, 2);
  const Stats = molstar.lib.structure.StructureElement.Stats;
  const pa = Stats.ofLoci(a.loci).center;
  const pb = Stats.ofLoci(b.loci).center;
  const distance = Math.hypot(pa[0] - pb[0], pa[1] - pb[1], pa[2] - pb[2]);
  const measurement = plugin.managers.structure.measurement;
  if (!measurement || typeof measurement.addDistance !== "function") {
    throw new Error("Mol* build is missing distance measurements");
  }
  await measurement.addDistance(a.loci, b.loci, {
    selectionTags: [VIEWER_DISTANCE_TAG],
    reprTags: [VIEWER_DISTANCE_TAG],
  });
  return distance;
}

async function clearMeasuredDistances(plugin) {
  return removeTaggedCells(plugin, VIEWER_DISTANCE_TAG);
}

// Mol* color-theme names behind the PyMOL coloring words. Element, chain, and
// spectrum recolor every loaded representation; named colors paint only the
// current selection (or the whole structure when nothing is selected).
const COLOR_THEMES = {
  element: "element-symbol",
  chain: "chain-id",
  spectrum: "sequence",
};

// PyMOL color words mapped to their RGB values (Mol* Color ints).
const PYMOL_COLORS = {
  red: 0xff0000,
  green: 0x00ff00,
  blue: 0x0000ff,
  yellow: 0xffff00,
  cyan: 0x00ffff,
  magenta: 0xff00ff,
  orange: 0xffa500,
  white: 0xffffff,
  gray: 0x808080,
  grey: 0x808080,
};

function pymolColor(name) {
  const color = PYMOL_COLORS[name];
  if (color === undefined) {
    throw new Error(ct("error.unsupportedColor", { name }));
  }
  return color;
}

// Switch every representation of every loaded component to a color theme.
async function setColorTheme(plugin, kind) {
  const theme = COLOR_THEMES[kind];
  if (!theme) throw new Error("unsupported color theme " + kind);
  const components = [];
  for (const entry of (plugin.managers.structure.hierarchy.current.structures) || []) {
    for (const component of entry.components || []) components.push(component);
  }
  if (!components.length) throw new Error(ct("error.loadBeforeColoring"));
  const componentManager = plugin.managers.structure.component;
  if (!componentManager || typeof componentManager.updateRepresentationsTheme !== "function") {
    throw new Error("Mol* build is missing theme updates");
  }
  await componentManager.updateRepresentationsTheme(components, { color: theme });
  return theme;
}

// Paint the current selection (or the whole structure) one solid color.
async function paintViewerSelection(molstar, plugin, name) {
  const color = pymolColor(name);
  const componentManager = plugin.managers.structure.component;
  if (!componentManager || typeof componentManager.applyTheme !== "function") {
    throw new Error("Mol* build is missing theme actions");
  }
  const StructureSelection = molstar.lib.structure.StructureSelection;
  const selectionManager = plugin.managers.structure.selection;
  const selection = {
    getSelection: async (_plugin, _ctx, structure) => {
      const picked = selectionManager && typeof selectionManager.getStructure === "function"
        ? selectionManager.getStructure(structure)
        : null;
      const target = picked && picked.elementCount ? picked : structure;
      return StructureSelection.Singletons(structure, target);
    },
  };
  const structures = (plugin.managers.structure.hierarchy.current.structures) || [];
  if (!structures.length) throw new Error(ct("error.loadBeforeColoring"));
  await componentManager.applyTheme({
    action: { name: "color", params: { color } },
    selection,
  }, structures);
  return color;
}

// Remove every solid-color paint layer this window applied, restoring the
// color theme underneath.
async function clearViewerPaint(molstar, plugin) {
  const componentManager = plugin.managers.structure.component;
  if (!componentManager || typeof componentManager.applyTheme !== "function") {
    throw new Error("Mol* build is missing theme actions");
  }
  const StructureSelection = molstar.lib.structure.StructureSelection;
  const selection = {
    getSelection: async (_plugin, _ctx, structure) => StructureSelection.Singletons(structure, structure),
  };
  const structures = (plugin.managers.structure.hierarchy.current.structures) || [];
  if (!structures.length) return 0;
  await componentManager.applyTheme({
    action: { name: "resetColor", params: {} },
    selection,
  }, structures);
  return structures.length;
}

// Selection-aware object for Mol*'s applyTheme: `picked` decides which
// sub-structure the action covers, falling back to the whole structure.
function themeSelectionOf(molstar, plugin, mode) {
  const StructureSelection = molstar.lib.structure.StructureSelection;
  const selectionManager = plugin.managers.structure && plugin.managers.structure.selection;
  return {
    getSelection: async (_plugin, _ctx, structure) => {
      if (mode === "whole") return StructureSelection.Singletons(structure, structure);
      const picked = selectionManager && typeof selectionManager.getStructure === "function"
        ? selectionManager.getStructure(structure)
        : null;
      const target = picked && picked.elementCount ? picked : structure;
      return StructureSelection.Singletons(structure, target);
    },
  };
}

async function applyTransparency(molstar, plugin, value, mode) {
  const componentManager = plugin.managers.structure.component;
  if (!componentManager || typeof componentManager.applyTheme !== "function") {
    throw new Error("Mol* build is missing theme actions");
  }
  const structures = (plugin.managers.structure.hierarchy.current.structures) || [];
  if (!structures.length) throw new Error(ct("error.loadBeforeHiding"));
  await componentManager.applyTheme({
    action: { name: "transparency", params: { value } },
    selection: themeSelectionOf(molstar, plugin, mode),
  }, structures);
}

// PyMOL `hide`/`show`: transparency layers make the chosen atoms invisible
// without deleting the underlying structure data. Later layers win, so
// hiding everything except the selection paints the whole structure opaque-
// first and then re-shows the selection on top.
async function setSelectionVisibility(molstar, plugin, mode) {
  if (mode === "hide") {
    await applyTransparency(molstar, plugin, 1, "selection");
    return ct("status.selectionHidden");
  }
  if (mode === "others") {
    await applyTransparency(molstar, plugin, 1, "whole");
    await applyTransparency(molstar, plugin, 0, "selection");
    return ct("status.othersHidden");
  }
  if (mode === "show") {
    await applyTransparency(molstar, plugin, 0, "whole");
    return ct("status.everythingShown");
  }
  throw new Error("unsupported visibility mode " + mode);
}

// The PyMOL verbs the command box understands, mapped to the toolbar actions.
const PYMOL_COMMANDS = ["select", "show", "hide", "color", "zoom", "center"];

// Grammar for one command-box line: `<verb> [argument][, selection]`.
// The comma matches PyMOL's split between the command argument (a
// representation or color word here) and the selection it applies to, e.g.
// `show cartoon, chain A` or `color red, resi 100`. `select`, `zoom`, and
// `center` take the whole remainder as their selection expression.
function parsePymolCommand(input) {
  const text = String(input || "").trim();
  if (!text) throw new Error(ct("error.emptyCommand"));
  const cut = text.search(/\s/);
  const command = cut === -1 ? text : text.slice(0, cut);
  if (PYMOL_COMMANDS.indexOf(command) === -1) {
    throw new Error(ct("error.outsideSubset", { verb: command }));
  }
  const rest = cut === -1 ? "" : text.slice(cut + 1).trim();
  if (command === "select" || command === "zoom" || command === "center") {
    return { command, argument: "", selection: rest };
  }
  const comma = rest.indexOf(",");
  const argument = (comma === -1 ? rest : rest.slice(0, comma)).trim();
  const selection = comma === -1 ? "" : rest.slice(comma + 1).trim();
  return { command, argument, selection };
}

// PyMOL spelling (plural or singular) for the representation kinds the
// show/hide buttons switch between.
const COMMAND_REPRESENTATIONS = {
  cartoon: "cartoon",
  stick: "stick",
  sticks: "stick",
  sphere: "sphere",
  spheres: "sphere",
  surface: "surface",
};

// PyMOL `center`: slide the camera target onto the current selection (or the
// whole structure when nothing is selected) without changing the distance.
function centerCurrentSelection(molstar, plugin) {
  const canvas = plugin.canvas3d;
  const camera = plugin.managers.camera;
  if (!canvas || !canvas.camera || !camera || typeof camera.setSnapshot !== "function") {
    throw new Error("viewer camera is not ready");
  }
  const lociList = currentSelectionLoci(molstar, plugin);
  let center;
  if (lociList.length) {
    const Stats = molstar.lib.structure.StructureElement.Stats;
    const sum = [0, 0, 0];
    for (const loci of lociList) {
      const each = Stats.ofLoci(loci).center;
      sum[0] += each[0];
      sum[1] += each[1];
      sum[2] += each[2];
    }
    center = scale(sum, 1 / lociList.length);
  } else {
    const sphere = canvas.boundingSphereVisible;
    if (!sphere) throw new Error(ct("error.loadBeforeCentering"));
    center = sphere.center;
  }
  const snapshot = cloneSnapshot(canvas.camera.getSnapshot());
  const delta = sub(center, snapshot.target);
  snapshot.target = center;
  snapshot.position = add(snapshot.position, delta);
  camera.setSnapshot(snapshot, 300);
  return lociList.length ? "selection" : "structure";
}

// Run one command-box line. A leading subset verb dispatches to the same
// actions the toolbar buttons drive. Anything else keeps the historical
// shortcut of treating the whole line as a selection expression; input that
// is neither a subset verb nor a compilable expression reports that it falls
// outside the subset.
async function runPymolCommand(molstar, plugin, input) {
  const text = String(input || "").trim();
  let parsed;
  try {
    parsed = parsePymolCommand(text);
  } catch (error) {
    try {
      const matched = highlightPymol(molstar, plugin, text);
      if (!matched) throw new Error(ct("error.noAtomsMatched", { expression: text }));
      return ct("cmd.selected", { expression: text });
    } catch (compileError) {
      const verb = text.split(/\s+/)[0];
      throw new Error(ct("error.outsideSubset", { verb }));
    }
  }
  const { command, argument, selection } = parsed;
  if (selection && command !== "select" && command !== "zoom" && command !== "center") {
    // `,<selection>` becomes the current selection first so the action that
    // follows paints exactly the atoms the user named.
    const matched = highlightPymol(molstar, plugin, selection);
    if (!matched) throw new Error(ct("error.noAtomsMatched", { expression: selection }));
  }
  if (command === "select") {
    if (!selection) throw new Error(ct("error.needsExpression"));
    const matched = highlightPymol(molstar, plugin, selection);
    if (!matched) throw new Error(ct("error.noAtomsMatched", { expression: selection }));
    return ct("cmd.selected", { expression: selection });
  }
  if (command === "show" || command === "hide") {
    if (!argument) throw new Error(ct("error.needsRepresentation", { command }));
    const kind = COMMAND_REPRESENTATIONS[argument.toLowerCase()];
    if (!kind) throw new Error(ct("error.unsupportedRepresentation", { name: argument }));
    await setStructureRepresentation(plugin, kind, command === "show");
    return command === "show"
      ? ct("cmd.shown", { representation: argument })
      : ct("cmd.hidden", { representation: argument });
  }
  if (command === "color") {
    if (!argument) throw new Error(ct("error.needsColor"));
    if (COLOR_THEMES[argument]) {
      await setColorTheme(plugin, argument);
      return ct("cmd.coloredByTheme", { theme: ct("color." + argument) });
    }
    pymolColor(argument);
    await paintViewerSelection(molstar, plugin, argument);
    return ct("cmd.coloredSolid", { color: ct("color." + argument) });
  }
  if (command === "zoom") {
    if (selection) {
      const matched = highlightPymol(molstar, plugin, selection);
      if (!matched) throw new Error(ct("error.noAtomsMatched", { expression: selection }));
      return ct("cmd.zoomedExpression", { expression: selection });
    }
    return focusCurrentSelection(molstar, plugin) === "selection"
      ? ct("cmd.zoomedSelection")
      : ct("cmd.zoomedStructure");
  }
  if (command === "center") {
    if (selection) {
      const matched = highlightPymol(molstar, plugin, selection);
      if (!matched) throw new Error(ct("error.noAtomsMatched", { expression: selection }));
    }
    return centerCurrentSelection(molstar, plugin) === "selection"
      ? ct("cmd.centeredSelection")
      : ct("cmd.centeredStructure");
  }
  throw new Error("unsupported command " + command);
}

const ViewerControls = {
  cloneSnapshot,
  zoomSnapshot,
  rotateSnapshot,
  highlightPymol,
  applyCameraPayload,
  setStructureRepresentation,
  REPRESENTATIONS,
  replaceLoadedStructure,
  clearLoadedStructures,
  clearStructureSelection,
  currentSelectionLoci,
  focusCurrentSelection,
  setSelectionLabels,
  measurePickedDistance,
  clearMeasuredDistances,
  COLOR_THEMES,
  PYMOL_COLORS,
  pymolColor,
  setColorTheme,
  paintViewerSelection,
  clearViewerPaint,
  setSelectionVisibility,
  parsePymolCommand,
  runPymolCommand,
  centerCurrentSelection,
  setStructureRepresentation,
  REPRESENTATIONS,
  CHROME_I18N,
  chromeText,
  normalizeChromeLocale,
  setChromeLocale,
};
globalThis.ViewerControls = ViewerControls;
if (typeof module !== "undefined" && module.exports) module.exports = ViewerControls;

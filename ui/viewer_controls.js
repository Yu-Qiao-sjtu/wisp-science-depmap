// Camera math and PyMOL highlight for the Mol* viewer window.
// Kept free of the DOM so the transforms can be tested without a GPU.

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
    throw new Error("select residues before labeling them");
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
    throw new Error("pick two atoms before measuring");
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
    throw new Error("unsupported color " + name + "; expected element, chain, spectrum, or a PyMOL color name");
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
  if (!components.length) throw new Error("load a structure before coloring it");
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
  if (!structures.length) throw new Error("load a structure before coloring it");
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
  if (!structures.length) throw new Error("load a structure before hiding parts of it");
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
    return "selection hidden";
  }
  if (mode === "others") {
    await applyTransparency(molstar, plugin, 1, "whole");
    await applyTransparency(molstar, plugin, 0, "selection");
    return "everything except the selection hidden";
  }
  if (mode === "show") {
    await applyTransparency(molstar, plugin, 0, "whole");
    return "everything shown";
  }
  throw new Error("unsupported visibility mode " + mode);
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
};
globalThis.ViewerControls = ViewerControls;
if (typeof module !== "undefined" && module.exports) module.exports = ViewerControls;

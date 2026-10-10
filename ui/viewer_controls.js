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

// PyMOL `label`: residue name+number labels on the current selection. Mol*'s
// label representation derives that text from each selected loci.
async function setSelectionLabels(molstar, plugin, visible) {
  const measurement = plugin.managers.structure && plugin.managers.structure.measurement;
  if (!measurement || typeof measurement.addLabel !== "function") {
    throw new Error("Mol* build is missing selection labels");
  }
  if (!visible) {
    const state = plugin.state.data;
    const build = state.build();
    let removed = 0;
    for (const [ref, cell] of state.cells) {
      const tags = cell.transform && cell.transform.tags;
      if (Array.isArray(tags) && tags.includes(VIEWER_LABEL_TAG)) {
        build.delete(ref);
        removed += 1;
      }
    }
    if (removed) await build.commit();
    return removed;
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
};
globalThis.ViewerControls = ViewerControls;
if (typeof module !== "undefined" && module.exports) module.exports = ViewerControls;

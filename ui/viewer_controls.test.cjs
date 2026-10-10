const assert = require("node:assert/strict");
const test = require("node:test");
const { rotateSnapshot, zoomSnapshot, setStructureRepresentation, replaceLoadedStructure, clearStructureSelection, focusCurrentSelection, setSelectionLabels, measurePickedDistance, clearMeasuredDistances } = require("./viewer_controls.js");

const start = {
  position: [0, 0, 10],
  target: [0, 0, 0],
  up: [0, 1, 0],
};

test("zoom moves the camera closer without moving the target", () => {
  const next = zoomSnapshot(start, 2);
  assert.deepEqual(next.target, [0, 0, 0]);
  assert.ok(Math.abs(next.position[2] - 5) < 1e-9);
  assert.deepEqual(start.position, [0, 0, 10]);
});

test("rotate turns the view around the target", () => {
  const next = rotateSnapshot(start, "y", 90);
  assert.ok(Math.abs(next.position[0] - 10) < 1e-6);
  assert.ok(Math.abs(next.position[2]) < 1e-6);
  assert.deepEqual(next.target, [0, 0, 0]);
});

test("stick can be turned on and cartoon turned off", async () => {
  const added = [];
  const removed = [];
  const cartoon = { cell: { params: { values: { type: { name: "cartoon" } } } } };
  const component = { representations: [cartoon] };
  const plugin = {
    managers: {
      structure: {
        hierarchy: {
          current: { structures: [{ components: [component] }] },
          remove(entries) { removed.push(entries); },
        },
        component: {
          addRepresentation(components, type) { added.push([components.length, type]); },
        },
      },
    },
  };
  assert.equal(await setStructureRepresentation(plugin, "stick", true), 1);
  assert.deepEqual(added, [[1, "ball-and-stick"]]);
  assert.equal(await setStructureRepresentation(plugin, "cartoon", false), 1);
  assert.deepEqual(removed, [[cartoon]]);
});

test("a new structure replaces the one already in the session", async () => {
  const removed = [];
  const loaded = [];
  const viewer = {
    plugin: {
      managers: {
        structure: {
          hierarchy: {
            current: { structures: [{ id: "previous" }] },
            remove(entries) {
              removed.push(entries.map((entry) => entry.id));
              this.current.structures = [];
            },
          },
        },
      },
    },
    loadStructureFromData(data, format) {
      loaded.push([data, format]);
    },
  };
  await replaceLoadedStructure(viewer, "ATOM", "pdb");
  assert.deepEqual(removed, [["previous"]]);
  assert.deepEqual(loaded, [["ATOM", "pdb"]]);
  assert.deepEqual(viewer.plugin.managers.structure.hierarchy.current.structures, []);
});

test("clearing a structure removes the selection and the highlight", () => {
  const calls = [];
  const plugin = {
    managers: {
      interactivity: {
        lociSelects: { deselectAll() { calls.push("deselect"); } },
        lociHighlights: { clearHighlights() { calls.push("clear"); } },
      },
    },
  };
  assert.equal(clearStructureSelection(plugin), true);
  assert.deepEqual(calls, ["deselect", "clear"]);
  assert.equal(clearStructureSelection({}), false);
});

test("focus frames the current selection, or the whole structure when empty", () => {
  const focused = [];
  const snapshots = [];
  const structure = {};
  const loci = { kind: "loci" };
  const plugin = {
    managers: {
      structure: {
        hierarchy: { current: { structures: [{ cell: { obj: { data: structure } } }] } },
        selection: { getLoci(s) { return s === structure ? loci : null; } },
      },
      interactivity: {},
      camera: { focusLoci(list, options) { focused.push([list, options]); } },
    },
    canvas3d: {
      camera: { getInvariantFocus: () => ({ radius: 5 }), getSnapshot: () => start },
      boundingSphereVisible: { center: [0, 0, 0], radius: 5 },
    },
  };
  const camera = plugin.managers.camera;
  camera.setSnapshot = (snapshot) => snapshots.push(snapshot);
  const molstar = { lib: { loci: { Loci: { isEmpty(l) { return !l; } } } } };
  assert.equal(focusCurrentSelection(molstar, plugin), "selection");
  assert.deepEqual(focused, [[[loci], { extraRadius: 2, durationMs: 300 }]]);
  // Empty selection falls back to framing the whole structure.
  plugin.managers.structure.selection.getLoci = () => null;
  assert.equal(focusCurrentSelection(molstar, plugin), "structure");
  assert.equal(focused.length, 1);
  assert.deepEqual(snapshots, [{ radius: 5 }]);
});

test("labels turn on for the selection and off again by tag", async () => {
  const structure = {};
  const loci = { structure };
  const labels = [];
  const deleted = [];
  const cells = new Map([
    ["keep", { transform: { tags: undefined }, obj: {} }],
    ["sel-1", { transform: { tags: ["wisp-viewer-label"] }, obj: {} }],
    ["repr-1", { transform: { tags: ["wisp-viewer-label"] }, obj: {} }],
  ]);
  const plugin = {
    managers: {
      structure: {
        hierarchy: { current: { structures: [{ cell: { obj: { data: structure } } }] } },
        selection: { getLoci: () => loci },
        measurement: {
          addLabel(l, options) { labels.push([l, options]); },
        },
      },
    },
    state: {
      data: {
        cells,
        build() {
          const ops = { delete(ref) { deleted.push(ref); }, commit: async () => {} };
          return ops;
        },
      },
    },
  };
  const molstar = { lib: { loci: { Loci: { isEmpty: (l) => !l } } } };
  assert.equal(await setSelectionLabels(molstar, plugin, true), 1);
  assert.deepEqual(labels, [[loci, {
    selectionTags: ["wisp-viewer-label"],
    reprTags: ["wisp-viewer-label"],
  }]]);
  // Off removes only the tagged cells, in one commit.
  assert.equal(await setSelectionLabels(molstar, plugin, false), 2);
  assert.deepEqual(deleted, ["sel-1", "repr-1"]);
});

test("labeling without a selection reports an error", async () => {
  const plugin = {
    managers: {
      structure: {
        hierarchy: { current: { structures: [] } },
        selection: {},
        measurement: { addLabel() {} },
      },
    },
  };
  const molstar = { lib: { loci: { Loci: { isEmpty: () => true } } } };
  await assert.rejects(() => setSelectionLabels(molstar, plugin, true), /select residues/);
});

test("distance uses the two most recent picks and draws the measurement", async () => {
  const distances = [];
  const deleted = [];
  const cells = new Map([
    ["d-repr", { transform: { tags: ["wisp-viewer-distance"] }, obj: {} }],
    ["d-sel", { transform: { tags: ["wisp-viewer-distance"] }, obj: {} }],
  ]);
  const structure = {};
  const plugin = {
    managers: {
      structure: {
        selection: {
          additionsHistory: [
            { loci: { structure, units: [] } },
            { loci: { structure, units: [] } },
            { loci: { structure, units: [] } },
          ],
        },
        measurement: {
          addDistance(a, b, options) { distances.push([a, b, options]); },
        },
      },
    },
    state: {
      data: {
        cells,
        build() { return { delete(ref) { deleted.push(ref); }, commit: async () => {} }; },
      },
    },
  };
  const centers = [[0, 0, 0], [3, 4, 0]];
  const molstar = { lib: { structure: { StructureElement: { Stats: { ofLoci(l) { return { center: l.center }; } } } } } };
  plugin.managers.structure.selection.additionsHistory[0].loci.center = centers[0];
  plugin.managers.structure.selection.additionsHistory[1].loci.center = centers[1];
  plugin.managers.structure.selection.additionsHistory[2].loci.center = centers[0];
  const distance = await measurePickedDistance(molstar, plugin);
  assert.equal(distance, 5);
  assert.equal(distances.length, 1);
  assert.deepEqual(distances[0][2], {
    selectionTags: ["wisp-viewer-distance"],
    reprTags: ["wisp-viewer-distance"],
  });
  assert.equal(await clearMeasuredDistances(plugin), 2);
  assert.deepEqual(deleted, ["d-repr", "d-sel"]);
});

test("measuring without two picks reports an error", async () => {
  const plugin = {
    managers: { structure: { selection: { additionsHistory: [{ loci: {} }] } } },
  };
  await assert.rejects(() => measurePickedDistance({ lib: {} }, plugin), /pick two atoms/);
});

test("vendored Mol* compiles a PyMOL selection", () => {
  const fs = require("node:fs");
  const vm = require("node:vm");
  const code = fs.readFileSync(require("node:path").join(__dirname, "vendor-src/molstar-viewer-5.12.0.js"), "utf8");
  const context = {
    console,
    Buffer,
    process,
    window: {},
    self: {},
    document: {
      createElement() { return { style: {}, setAttribute() {}, appendChild() {} }; },
      querySelector() { return null; },
    },
  };
  context.window = context;
  context.self = context;
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(code, context, { timeout: 60000 });
  const query = context.molstar.scriptToQuery({
    language: "pymol",
    expression: "name CA and polymer.protein",
  });
  assert.equal(typeof query, "function");
  assert.equal(typeof context.molstar.scriptToQuery({ language: "pymol", expression: "organic" }), "function");
  assert.throws(() => context.molstar.scriptToQuery({ language: "pymol", expression: "not a selection !!!" }));
});

const assert = require("node:assert/strict");
const test = require("node:test");
const { rotateSnapshot, zoomSnapshot, replaceLoadedStructure, clearStructureSelection } = require("./viewer_controls.js");

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

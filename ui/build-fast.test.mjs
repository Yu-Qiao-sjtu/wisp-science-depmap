import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, statSync, existsSync, readdirSync, rmSync, utimesSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { test } from "node:test";
import { buildFast, stableModulePreloads, syncAssets } from "./build-fast.mjs";

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "wisp fast assets "));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const ui = join(root, "ui");
  const source = join(root, "stage");
  const dist = join(ui, "dist-fast");
  for (const path of [ui, source, dist]) mkdirSync(path, { recursive: true });
  return { root, ui, source, dist };
}

test("Trunk's shuffled module preload hints stabilize without reordering CSS or scripts", () => {
  const a = '<link rel="modulepreload" href="/a.js" integrity="sha384-a">';
  const b = '<link rel="modulepreload" href="/b.js" integrity="sha384-b">';
  const prefix = '<link rel="stylesheet" href="z.css"><link rel="stylesheet" href="a.css">';
  const suffix = '<script src="z.js"></script><script src="a.js"></script>';
  assert.equal(stableModulePreloads(prefix + b + a + suffix), prefix + a + b + suffix);
  assert.equal(stableModulePreloads(prefix + a + b + suffix), prefix + a + b + suffix);
  assert.equal(stableModulePreloads(a + suffix + b), a + suffix + b);
  assert.notEqual(stableModulePreloads(a + b), stableModulePreloads(a + b.replace("sha384-b", "sha384-c")));
});

test("unchanged embedded assets keep their timestamps; changed and removed assets update", (t) => {
  const { source, dist } = fixture(t);
  writeFileSync(join(source, "index.html"), "new wasm hash");
  mkdirSync(join(source, "fonts"));
  writeFileSync(join(source, "fonts/font.woff2"), "unchanged font");
  syncAssets(source, dist);
  const font = join(dist, "fonts/font.woff2");
  utimesSync(font, 1, 1);
  const unchangedTime = statSync(font).mtimeMs;
  writeFileSync(join(dist, "index.html"), "old wasm hash");
  writeFileSync(join(dist, "old-hash.wasm"), "obsolete");
  syncAssets(source, dist);
  assert.equal(statSync(font).mtimeMs, unchangedTime);
  assert.equal(readFileSync(join(dist, "index.html"), "utf8"), "new wasm hash");
  assert.equal(existsSync(join(dist, "old-hash.wasm")), false);
});

test("file/directory replacements remove stale nested assets", (t) => {
  const { source, dist } = fixture(t);
  mkdirSync(join(source, "nested"));
  writeFileSync(join(source, "nested/current"), "current");
  writeFileSync(join(dist, "nested"), "formerly a file");
  writeFileSync(join(source, "flat"), "now a file");
  mkdirSync(join(dist, "flat"));
  writeFileSync(join(dist, "flat/stale"), "stale");
  syncAssets(source, dist);
  assert.equal(readFileSync(join(dist, "nested/current"), "utf8"), "current");
  assert.equal(readFileSync(join(dist, "flat"), "utf8"), "now a file");
});

test("a failed compiler leaves the last built frontend intact and cleans staging", (t) => {
  const { root, ui, dist } = fixture(t);
  writeFileSync(join(dist, "index.html"), "previous successful build");
  assert.throws(() => buildFast(ui, (program, args) => {
    if (program !== "trunk") return;
    writeFileSync(join(args[args.indexOf("--dist") + 1], "index.html"), "incomplete");
    throw new Error("compiler failed");
  }), /compiler failed/);
  assert.equal(readFileSync(join(dist, "index.html"), "utf8"), "previous successful build");
  assert.deepEqual(readdirSync(join(root, "target")), []);
});

test("every build runs the compiler without watching or release optimization", (t) => {
  const { root, ui, dist } = fixture(t);
  for (const key of ["NO_COLOR", "TRUNK_NO_COLOR"]) {
    const original = process.env[key];
    t.after(() => {
      if (original === undefined) delete process.env[key];
      else process.env[key] = original;
    });
    process.env[key] = "1";
  }
  let calls = 0;
  const fakeCompiler = (program, args, options) => {
    assert.equal(options.cwd, ui);
    assert.equal(options.env.NO_COLOR, undefined);
    assert.equal(options.env.TRUNK_NO_COLOR, undefined);
    if (program !== "trunk") return;
    calls++;
    assert.equal(args[0], "build");
    assert.ok(args.includes("--release=false"));
    assert.equal(args[args.indexOf("--cargo-profile") + 1], "dev");
    writeFileSync(join(args[args.indexOf("--dist") + 1], "index.html"), "built");
  };
  buildFast(ui, fakeCompiler);
  const file = join(dist, "index.html");
  utimesSync(file, 1, 1);
  const timestamp = statSync(file).mtimeMs;
  buildFast(ui, fakeCompiler);
  assert.equal(calls, 2);
  assert.equal(process.env.NO_COLOR, "1");
  assert.equal(process.env.TRUNK_NO_COLOR, "1");
  assert.equal(statSync(file).mtimeMs, timestamp);
  assert.deepEqual(readdirSync(join(root, "target")), []);
});

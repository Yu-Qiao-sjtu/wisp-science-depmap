// Keep unchanged embedded assets' mtimes: Tauri tracks them as Rust inputs.
// Trunk still checks every build input through Cargo on every invocation.
import {
  copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync,
  readFileSync, readdirSync, rmSync, writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export function stableModulePreloads(html) {
  // Trunk emits snippet preload hints in hash-map order. Sort only consecutive
  // modulepreload hints; stylesheet/script order must remain untouched.
  return html.replace(/(?:<link\b[^>]*\brel="modulepreload"[^>]*>\s*){2,}/g,
    (block) => block.match(/<link\b[^>]*\brel="modulepreload"[^>]*>/g).sort().join(""));
}

export function syncAssets(source, destination) {
  if (existsSync(destination) && !lstatSync(destination).isDirectory()) {
    rmSync(destination, { recursive: true, force: true });
  }
  mkdirSync(destination, { recursive: true });
  const entries = readdirSync(source, { withFileTypes: true });
  const names = new Set(entries.map((entry) => entry.name));
  for (const name of readdirSync(destination)) {
    if (!names.has(name)) rmSync(join(destination, name), { recursive: true, force: true });
  }
  for (const entry of entries) {
    const from = join(source, entry.name);
    const to = join(destination, entry.name);
    if (entry.isDirectory()) {
      syncAssets(from, to);
    } else if (entry.isFile()) {
      if (existsSync(to)) {
        if (lstatSync(to).isFile()) {
          if (readFileSync(from).equals(readFileSync(to))) continue;
        } else {
          rmSync(to, { recursive: true, force: true });
        }
      }
      copyFileSync(from, to);
    } else {
      throw new Error(`Unexpected frontend asset type: ${from}`);
    }
  }
}

function runCommand(program, args, options) {
  const result = spawnSync(program, args, { ...options, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${program} failed (${result.signal ?? result.status})`);
}

export function buildFast(ui, run = runCommand) {
  const target = join(ui, "..", "target");
  mkdirSync(target, { recursive: true });
  const staging = mkdtempSync(join(target, "ui-fast-stage-"));
  const env = { ...process.env };
  // Trunk rejects the conventional NO_COLOR=1 (it expects a bool).
  delete env.NO_COLOR;
  delete env.TRUNK_NO_COLOR;
  try {
    run(process.execPath, [join(ui, "sync-vendor.mjs")], { cwd: ui, env });
    run("trunk", [
      "build", "--locked", "--release=false", "--cargo-profile", "dev",
      "--dist", staging, "--skip-version-check",
    ], { cwd: ui, env });
    const index = join(staging, "index.html");
    writeFileSync(index, stableModulePreloads(readFileSync(index, "utf8")));
    syncAssets(staging, join(ui, "dist-fast"));
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    buildFast(dirname(fileURLToPath(import.meta.url)));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

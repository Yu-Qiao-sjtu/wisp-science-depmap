import { cp, mkdir, readdir, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = path.join(root, "dist-cloudflare");
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
// Explicit public files only: never publish tooling, credentials or test output.
for (const entry of await readdir(root, { withFileTypes: true })) {
  if (entry.isFile() && entry.name.endsWith(".html")) {
    await cp(path.join(root, entry.name), path.join(output, entry.name));
  }
}
for (const directory of ["assets", "tutorials"]) {
  await cp(path.join(root, directory), path.join(output, directory), { recursive: true });
}
await cp(path.join(root, "skills-catalog.json"), path.join(output, "skills-catalog.json"));

import { existsSync, rmSync, statSync } from "node:fs";
import { join } from "node:path";
import { cwd } from "node:process";
import { spawnSync } from "node:child_process";

const root = cwd();
const dist = join(root, "dist");
const out = join(root, "terminalbuddy.xdc");

if (!existsSync(join(dist, "index.html"))) {
  console.error("dist/index.html missing — run vite build first");
  process.exit(1);
}

// zip updates existing archives by default — always recreate.
if (existsSync(out)) {
  rmSync(out);
}

const result = spawnSync(
  "bash",
  ["-lc", `cd "${dist}" && zip -9 -r -q "${out}" .`],
  { stdio: "inherit" },
);

if (result.status !== 0) {
  console.error("zip failed — is zip installed?");
  process.exit(result.status ?? 1);
}

const size = statSync(out).size;
console.log(`Packed ${out} (${size} bytes)`);
if (size > 640 * 1024) {
  console.warn("Warning: .xdc is over the common 640 KiB webxdc size limit");
}

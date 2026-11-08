import { copyFile, mkdir } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Copies what `cargo xtask perf-wasm` wrote into the app: the wasm becomes a static asset under
// public/perf/, the JS wrapper is bundled from lib/generated/ (docs/perf-platform.md §7.1).
const here = dirname(fileURLToPath(import.meta.url));
const app = resolve(here, "..");
const repo = resolve(app, "..", "..");
const from = process.env.PERF_WASM_DIR ?? join(repo, "target", "perf-wasm");
const wasm = join(app, "public", "perf", "horizon_perf.wasm");
const wrapper = join(app, "lib", "generated", "horizon_perf.js");

const fonts = join(repo, "crates", "horizon-theme", "fonts");
const copies = [
  [join(from, "horizon_perf.wasm"), wasm],
  [join(from, "horizon_perf.js"), wrapper],
  ...["Geist-Regular", "Geist-Medium", "Geist-SemiBold", "GeistMono-Regular"].map((name) => [
    join(fonts, `${name}.ttf`),
    join(app, "public", "perf", "fonts", `${name}.ttf`),
  ]),
];

for (const [source, target] of copies) {
  await mkdir(dirname(target), { recursive: true });
  try {
    await copyFile(source, target);
  } catch {
    console.error(
      `missing ${source}\nrun \`cargo xtask perf-wasm\` first, or set PERF_WASM_DIR to the folder it wrote`,
    );
    process.exit(1);
  }
}
console.log(`copied horizon_perf.wasm and horizon_perf.js from ${from}`);

// Builds the extension into dist/ (D34): esbuild bundles, static files, the
// WASM core from `just wasm`, and CSS from the design tokens. Fails on any
// manifest or remote-code problem.
//
//   node scripts/build.mjs

import { build } from "esbuild";
import console from "node:console";
import process from "node:process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { checkManifest, remoteUrls } from "./check.mjs";
import { tokensCss } from "./tokens.mjs";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const dist = path.join(root, "dist");
const core = path.join(root, "core");

function fail(message) {
  console.error(`build: ${message}`);
  process.exit(1);
}

if (!fs.existsSync(path.join(core, "pwm_wasm.js")))
  fail("no WASM core in extension/core; run `just wasm` first");

fs.rmSync(dist, { recursive: true, force: true });
fs.mkdirSync(dist);

await build({
  absWorkingDir: root,
  entryPoints: {
    background: "src/background/index.ts",
    popup: "src/popup/main.tsx",
    offscreen: "src/offscreen/index.ts",
  },
  outdir: dist,
  bundle: true,
  format: "esm",
  target: "chrome116",
  jsx: "automatic",
  jsxImportSource: "preact",
  alias: { "pwm-core": path.join(core, "pwm_wasm.js") },
  minify: true,
  legalComments: "none",
  logLevel: "warning",
});

for (const file of fs.readdirSync(path.join(root, "static"))) {
  fs.copyFileSync(path.join(root, "static", file), path.join(dist, file));
}
fs.copyFileSync(
  path.join(core, "pwm_wasm_bg.wasm"),
  path.join(dist, "pwm_wasm_bg.wasm"),
);
const tokens = JSON.parse(
  fs.readFileSync(path.join(root, "..", "design", "tokens.json"), "utf8"),
);
fs.writeFileSync(path.join(dist, "tokens.css"), tokensCss(tokens));

const pkg = JSON.parse(
  fs.readFileSync(path.join(root, "package.json"), "utf8"),
);
const manifest = JSON.parse(
  fs.readFileSync(path.join(dist, "manifest.json"), "utf8"),
);
const problems = checkManifest(manifest, pkg.version);
for (const file of fs.readdirSync(dist).filter((f) => f.endsWith(".js"))) {
  for (const url of remoteUrls(fs.readFileSync(path.join(dist, file), "utf8")))
    problems.push(`${file}: remote URL ${url}`);
}
if (problems.length > 0) fail(problems.join("\n  "));
console.log(`build: dist/ ready (${fs.readdirSync(dist).length} files)`);

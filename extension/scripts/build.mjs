// Builds the extension into dist/ (D34, D35): esbuild bundles, static
// files, the WASM core from `just wasm`, and CSS from the design tokens.
// Fails on any manifest or remote-code problem.
//
//   node scripts/build.mjs          release build into dist/
//   node scripts/build.mjs --test   the browser tests' build into dist-test/:
//                                   site access granted at install and a
//                                   test hook on the background (never shipped)

import { build } from "esbuild";
import console from "node:console";
import process from "node:process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { OPTIONAL_HOSTS, checkManifest, remoteUrls } from "./check.mjs";
import { tokensCss } from "./tokens.mjs";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const test = process.argv.includes("--test");
const dist = path.join(root, test ? "dist-test" : "dist");
const define = { PWM_TEST: String(test) };
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
    inline: "src/inline/index.ts",
    passkey: "src/passkeys/window.ts",
  },
  outdir: dist,
  bundle: true,
  format: "esm",
  target: "chrome127",
  jsx: "automatic",
  jsxImportSource: "preact",
  alias: { "pwm-core": path.join(core, "pwm_wasm.js") },
  define,
  minify: true,
  legalComments: "none",
  logLevel: "warning",
});

// Injected with chrome.scripting, which runs classic scripts: one IIFE.
await build({
  absWorkingDir: root,
  entryPoints: {
    content: "src/content/index.ts",
    "passkey-page": "src/passkeys/page.ts",
    "passkey-bridge": "src/passkeys/bridge.ts",
  },
  outdir: dist,
  bundle: true,
  format: "iife",
  define,
  target: "chrome127",
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
const manifestPath = path.join(dist, "manifest.json");
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const problems = test ? [] : checkManifest(manifest, pkg.version);
if (test) {
  // Playwright cannot click the toolbar button (activeTab) or answer a
  // permission prompt, so the test build has site access from install.
  manifest.host_permissions.push(...OPTIONAL_HOSTS);
  manifest.name = "PWM (test build)";
  fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
}
for (const file of fs.readdirSync(dist).filter((f) => f.endsWith(".js"))) {
  for (const url of remoteUrls(fs.readFileSync(path.join(dist, file), "utf8")))
    problems.push(`${file}: remote URL ${url}`);
}
if (!test) {
  for (const file of fs.readdirSync(dist).filter((f) => f.endsWith(".js"))) {
    if (fs.readFileSync(path.join(dist, file), "utf8").includes("pwmTest"))
      problems.push(`${file}: the test hook is in a release build`);
  }
}
if (problems.length > 0) fail(problems.join("\n  "));
console.log(
  `build: ${path.basename(dist)}/ ready (${String(fs.readdirSync(dist).length)} files)`,
);

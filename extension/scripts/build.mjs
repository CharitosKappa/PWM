// Builds the extension (D34, D35, D40): esbuild bundles, static files, the
// WASM core from `just wasm`, CSS from the design tokens and the target's
// manifest, then a zip for the stores. Fails on any manifest or remote-code
// problem.
//
//   node scripts/build.mjs                    every target into dist/<target>/
//   node scripts/build.mjs --target firefox   one target
//   node scripts/build.mjs --test             the browser tests' build into
//                                             dist-test/ (Chromium): site access
//                                             at install and a test hook on the
//                                             background, never shipped
//
// Chromium is one package for Chrome, Edge, Brave and Opera. Firefox's is
// not minified, so AMO reviewers can read it. Safari's goes inside the macOS
// app (D48 §4), which copies dist/safari/ in when Xcode builds it.

import { build } from "esbuild";
import { zipSync } from "fflate";
import console from "node:console";
import process from "node:process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  OPTIONAL_HOSTS,
  TARGETS,
  checkManifest,
  remoteUrls,
} from "./check.mjs";
import { copyFonts, tokensCss } from "./tokens.mjs";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const core = path.join(root, "core");
const test = process.argv.includes("--test");
const only = process.argv.includes("--target")
  ? process.argv[process.argv.indexOf("--target") + 1]
  : undefined;

function fail(message) {
  console.error(`build: ${message}`);
  process.exit(1);
}

if (only !== undefined && !TARGETS.includes(only))
  fail(`unknown target ${only}`);
if (!fs.existsSync(path.join(core, "pwm_wasm.js")))
  fail("no WASM core in extension/core; run `just wasm` first");

const pkg = JSON.parse(
  fs.readFileSync(path.join(root, "package.json"), "utf8"),
);

/** Static files only one target uses. */
const ONLY_FOR = {
  "offscreen.html": "chromium",
  "inline.html": "chromium",
  "inline.css": "chromium",
};

/** Every file under `dir`, relative to it, for the zip. */
function files(dir, base = dir, out = {}) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) files(full, base, out);
    else
      out[path.relative(base, full).split(path.sep).join("/")] =
        fs.readFileSync(full);
  }
  return out;
}

async function buildTarget(target, dist) {
  const chromium = target === "chromium";
  const readable = target === "firefox";
  const define = { PWM_TEST: String(test), PWM_TARGET: JSON.stringify(target) };
  const common = {
    absWorkingDir: root,
    outdir: dist,
    bundle: true,
    define,
    target: {
      chromium: "chrome127",
      firefox: "firefox128",
      safari: "safari17",
    }[target],
    // Firefox stays readable for AMO review, but its dead code (the test
    // hook) is still removed.
    minifySyntax: true,
    minifyWhitespace: !readable,
    minifyIdentifiers: !readable,
    legalComments: "none",
    logLevel: "warning",
  };
  fs.rmSync(dist, { recursive: true, force: true });
  fs.mkdirSync(dist, { recursive: true });

  await build({
    ...common,
    entryPoints: {
      background: "src/background/index.ts",
      popup: "src/popup/main.tsx",
      manage: "src/manage/main.tsx",
      passkey: "src/passkeys/window.ts",
      ...(chromium
        ? { offscreen: "src/offscreen/index.ts", inline: "src/inline/index.ts" }
        : {}),
    },
    format: "esm",
    jsx: "automatic",
    jsxImportSource: "preact",
    alias: { "pwm-core": path.join(core, "pwm_wasm.js") },
  });
  // Injected with chrome.scripting, which runs classic scripts: one IIFE each.
  await build({
    ...common,
    entryPoints: {
      content: "src/content/index.ts",
      "passkey-page": "src/passkeys/page.ts",
      "passkey-bridge": "src/passkeys/bridge.ts",
    },
    format: "iife",
  });

  for (const file of fs.readdirSync(path.join(root, "static"))) {
    if (ONLY_FOR[file] !== undefined && ONLY_FOR[file] !== target) continue;
    fs.copyFileSync(path.join(root, "static", file), path.join(dist, file));
  }
  // The shared screens' styles (D3 §2), under the name the pages load.
  fs.copyFileSync(
    path.join(root, "..", "ui", "src", "screens.css"),
    path.join(dist, "popup.css"),
  );
  fs.copyFileSync(
    path.join(core, "pwm_wasm_bg.wasm"),
    path.join(dist, "pwm_wasm_bg.wasm"),
  );
  const tokens = JSON.parse(
    fs.readFileSync(path.join(root, "..", "design", "tokens.json"), "utf8"),
  );
  fs.writeFileSync(path.join(dist, "tokens.css"), tokensCss(tokens));
  copyFonts(path.join(root, ".."), dist);

  const manifest = JSON.parse(
    fs.readFileSync(path.join(root, "manifests", `${target}.json`), "utf8"),
  );
  const problems = test ? [] : checkManifest(manifest, pkg.version, target);
  if (test) {
    // Playwright cannot click the toolbar button (activeTab) or answer a
    // permission prompt, so the test build has site access from install.
    manifest.host_permissions.push(...OPTIONAL_HOSTS);
    manifest.name = "PWM (test build)";
  }
  fs.writeFileSync(
    path.join(dist, "manifest.json"),
    JSON.stringify(manifest, null, 2) + "\n",
  );

  for (const file of fs.readdirSync(dist).filter((f) => f.endsWith(".js"))) {
    const source = fs.readFileSync(path.join(dist, file), "utf8");
    for (const url of remoteUrls(source))
      problems.push(`${file}: remote URL ${url}`);
    if (!test && source.includes("pwmTest"))
      problems.push(`${file}: the test hook is in a release build`);
  }
  if (problems.length > 0) fail(`${target}: ${problems.join("\n  ")}`);
  console.log(
    `build: ${path.relative(root, dist)}/ ready (${String(fs.readdirSync(dist).length)} files)`,
  );
  if (!test) {
    const zip = path.join(
      path.dirname(dist),
      `pwm-${target}-${pkg.version}.zip`,
    );
    fs.writeFileSync(zip, zipSync(files(dist), { level: 9 }));
    console.log(`build: ${path.relative(root, zip)}`);
  }
}

if (test) {
  await buildTarget("chromium", path.join(root, "dist-test"));
} else {
  if (only === undefined)
    fs.rmSync(path.join(root, "dist"), { recursive: true, force: true });
  for (const target of only === undefined ? TARGETS : [only]) {
    await buildTarget(target, path.join(root, "dist", target));
  }
}

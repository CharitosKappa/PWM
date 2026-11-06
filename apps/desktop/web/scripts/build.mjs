// Builds the desktop app's page (D66 §2) into dist/, which Tauri serves:
// the shared screens with the desktop host, the design tokens' CSS and the
// screens' styles. Run before `cargo build` in apps/desktop.

import { build } from "esbuild";
import console from "node:console";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { copyFonts, tokensCss } from "../../../../extension/scripts/tokens.mjs";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const repo = path.join(root, "..", "..", "..");
const dist = path.join(root, "dist");

fs.rmSync(dist, { recursive: true, force: true });
fs.mkdirSync(dist, { recursive: true });

await build({
  absWorkingDir: root,
  entryPoints: { app: "src/main.tsx", manage: "src/manage.tsx" },
  outdir: dist,
  bundle: true,
  format: "esm",
  target: "es2022",
  jsx: "automatic",
  jsxImportSource: "preact",
  minifySyntax: true,
  minifyWhitespace: true,
  legalComments: "none",
  logLevel: "warning",
});

for (const page of ["index.html", "manage.html"]) {
  fs.copyFileSync(path.join(root, "static", page), path.join(dist, page));
}
fs.copyFileSync(
  path.join(repo, "ui", "src", "screens.css"),
  path.join(dist, "screens.css"),
);
const tokens = JSON.parse(
  fs.readFileSync(path.join(repo, "design", "tokens.json"), "utf8"),
);
fs.writeFileSync(path.join(dist, "tokens.css"), tokensCss(tokens));
copyFonts(repo, dist);
console.log(`build: ${path.relative(process.cwd(), dist)} ready`);

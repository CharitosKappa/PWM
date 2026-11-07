// Builds the desktop installers (D70): the page, then on Windows the browser
// helper under the name Tauri's `externalBin` expects, then `tauri build`
// with the installer settings. Arguments after the script go to `tauri
// build`, such as `--config sign.json` for a sign command.
//
// Windows: the per-user NSIS installer. Linux: the .deb and the .rpm. The
// installers land in apps/desktop/target/release/bundle/.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const web = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const desktop = path.dirname(web);
const tauri = path.join(desktop, "src-tauri");
const windows = process.platform === "win32";

function run(command, args, cwd) {
  execFileSync(command, args, { cwd, stdio: "inherit" });
}

run(process.execPath, [path.join(web, "scripts", "build.mjs")], web);

const configs = ["--config", path.join(tauri, "installer.conf.json")];
if (windows) {
  // The helper goes next to the app (D69 §4, D70 §4).
  run(
    "cargo",
    ["build", "--release", "--locked", "-p", "pwm-browser-helper"],
    desktop,
  );
  const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(
    /^host: (\S+)$/m,
  );
  if (!host) throw new Error("rustc -vV gave no host triple");
  const binaries = path.join(tauri, "binaries");
  fs.mkdirSync(binaries, { recursive: true });
  fs.copyFileSync(
    path.join(desktop, "target", "release", "pwm-browser-helper.exe"),
    path.join(binaries, `pwm-browser-helper-${host[1]}.exe`),
  );
  configs.push("--config", path.join(tauri, "installer.windows.conf.json"));
}

const cli = path.join(web, "node_modules", "@tauri-apps", "cli", "tauri.js");
const bundles = windows ? "nsis" : "deb,rpm";
const extra = process.argv.slice(2);
run(
  process.execPath,
  [
    cli,
    "build",
    "--ci",
    "--bundles",
    bundles,
    ...configs,
    ...extra,
    "--",
    "--locked",
  ],
  desktop,
);

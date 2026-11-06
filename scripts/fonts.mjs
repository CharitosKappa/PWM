// Copies the app fonts (design/fonts, D1) into the projects that bundle them
// from fixed paths: the Apple package's resources and the Android app's
// res/font. The extension and the desktop app copy them at build time.
//
//   node scripts/fonts.mjs            write the copies
//   node scripts/fonts.mjs --check    exit 1 when a copy is missing or differs
//   node scripts/fonts.mjs --notices  also write THIRD_PARTY_NOTICES/fonts.md:
//                                     NOTICE.md and both licence texts

import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SOURCE = join(ROOT, "design", "fonts");

/** [file in design/fonts, copy] — Android resource names must be lower case. */
export const COPIES = [
  ["Manrope.ttf", "apps/apple/PWMKit/Sources/PWMDesign/Fonts/Manrope.ttf"],
  ["GeistMono.ttf", "apps/apple/PWMKit/Sources/PWMDesign/Fonts/GeistMono.ttf"],
  ["Manrope.ttf", "apps/android/app/src/main/res/font/manrope.ttf"],
  ["GeistMono.ttf", "apps/android/app/src/main/res/font/geist_mono.ttf"],
];

const check = process.argv.includes("--check");
let stale = 0;
for (const [name, target] of COPIES) {
  const from = join(SOURCE, name);
  const to = join(ROOT, target);
  const same = existsSync(to) && readFileSync(from).equals(readFileSync(to));
  if (check) {
    if (!same) {
      console.error(`stale: ${target} (run node scripts/fonts.mjs)`);
      stale++;
    }
  } else if (!same) {
    mkdirSync(dirname(to), { recursive: true });
    copyFileSync(from, to);
    console.log(`wrote ${target}`);
  }
}
if (stale > 0) process.exit(1);

if (process.argv.includes("--notices")) {
  const read = (name) => readFileSync(join(SOURCE, name), "utf8").trimEnd();
  const text = [
    read("NOTICE.md"),
    "## Licence text: Manrope\n\n" + read("OFL-Manrope.txt"),
    "## Licence text: Geist Mono\n\n" + read("OFL-GeistMono.txt"),
  ].join("\n\n") + "\n";
  const out = join(ROOT, "THIRD_PARTY_NOTICES", "fonts.md");
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, text);
  console.log(`wrote ${out}`);
}

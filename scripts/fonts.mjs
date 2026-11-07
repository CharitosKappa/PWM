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

/** The bundled fonts, from `$extensions["pwm.font"]` in design/tokens.json. */
const TOKENS = JSON.parse(readFileSync(join(ROOT, "design", "tokens.json"), "utf8"));
const FONTS = Object.entries(TOKENS.font.family)
  .filter(([key]) => !key.startsWith("$"))
  .map(([key, token]) => {
    const ext = token.$extensions?.["pwm.font"];
    if (!ext?.file || !ext.licence) throw new Error(`tokens.json: font.family.${key} has no pwm.font file`);
    return { family: token.$value[0], file: ext.file, licence: ext.licence };
  });

/** `GeistMono.ttf` → `geist_mono`: Android resource names are lower case.
 *  scripts/tokens-kotlin.mjs uses the same rule for `R.font`. */
const androidName = (file) =>
  file.replace(/\.ttf$/, "").replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase();

/** [file in design/fonts, copy] */
export const COPIES = FONTS.flatMap((f) => [
  [f.file, `apps/apple/PWMKit/Sources/PWMDesign/Fonts/${f.file}`],
  [f.file, `apps/android/app/src/main/res/font/${androidName(f.file)}.ttf`],
]);

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
    ...FONTS.map((f) => `## Licence text: ${f.family}\n\n` + read(f.licence)),
  ].join("\n\n") + "\n";
  const out = join(ROOT, "THIRD_PARTY_NOTICES", "fonts.md");
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, text);
  console.log(`wrote ${out}`);
}

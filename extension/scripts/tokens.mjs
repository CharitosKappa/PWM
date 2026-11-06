// CSS custom properties from design/tokens.json (W3C Design Tokens).
// Light values on :root, dark ones under prefers-color-scheme.

import fs from "node:fs";
import path from "node:path";

/**
 * The bundled app fonts (design/fonts, D1): each family's file and the
 * weights its variable font covers. Pages load them from `fonts/`.
 */
export const FONT_FILES = [
  { family: "Manrope", file: "Manrope.ttf", weights: "200 800" },
  { family: "Geist Mono", file: "GeistMono.ttf", weights: "100 900" },
];

/**
 * Copies the font files with their licence texts and NOTICE.md (OFL: the
 * licence ships with the fonts) into `dist/fonts`.
 * @param {string} repo the repository root
 * @param {string} dist the build's output folder
 */
export function copyFonts(repo, dist) {
  const from = path.join(repo, "design", "fonts");
  const to = path.join(dist, "fonts");
  fs.mkdirSync(to, { recursive: true });
  const names = [
    ...FONT_FILES.map((f) => f.file),
    "OFL-Manrope.txt",
    "OFL-GeistMono.txt",
    "NOTICE.md",
  ];
  for (const name of names)
    fs.copyFileSync(path.join(from, name), path.join(to, name));
}

/** @param {unknown} value */
function px(value) {
  const dim = /** @type {{ value: number, unit: string }} */ (value);
  return `${String(dim.value)}${dim.unit}`;
}

/**
 * @param {Record<string, any>} tokens the parsed tokens.json
 * @returns {string}
 */
export function tokensCss(tokens) {
  if (tokens.version?.$value !== 1)
    throw new Error("tokens.json: unknown version");
  /** @param {Record<string, any>} group @param {(t: any) => string} f @param {string} prefix */
  const lines = (group, prefix, f) =>
    Object.entries(group)
      .filter(([name]) => !name.startsWith("$"))
      .map(([name, token]) => `  --${prefix}-${name}: ${f(token.$value)};`);
  const fonts = (/** @type {string[]} */ list) =>
    list.map((f) => (f.includes(" ") ? `"${f}"` : f)).join(", ");
  const root = [
    ...lines(tokens.color.light, "color", String),
    `  --font-sans: ${fonts(tokens.font.family.sans.$value)};`,
    `  --font-mono: ${fonts(tokens.font.family.mono.$value)};`,
    ...lines(tokens.font.weight, "font", String),
    ...Object.entries(tokens.type).flatMap(([name, token]) => [
      `  --type-${name}-size: ${px(token.$value.fontSize)};`,
      `  --type-${name}-line: ${String(token.$value.lineHeight)};`,
    ]),
    ...lines(tokens.space, "space", px),
    ...lines(tokens.radius, "radius", (v) =>
      typeof v === "object" ? px(v) : String(v),
    ),
  ];
  const dark = lines(tokens.color.dark, "color", String).map((l) => `  ${l}`);
  const faces = FONT_FILES.flatMap((f) => [
    "@font-face {",
    `  font-family: "${f.family}";`,
    `  src: url("fonts/${f.file}") format("truetype");`,
    `  font-weight: ${f.weights};`,
    "  font-style: normal;",
    "  font-display: swap;",
    "}",
  ]);
  return [
    "/* Generated from design/tokens.json by scripts/build.mjs. Do not edit. */",
    ...faces,
    ":root {",
    "  color-scheme: light dark;",
    ...root,
    "}",
    "@media (prefers-color-scheme: dark) {",
    "  :root {",
    ...dark,
    "  }",
    "}",
    "",
  ].join("\n");
}

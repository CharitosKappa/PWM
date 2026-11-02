// CSS custom properties from design/tokens.json (W3C Design Tokens).
// Light values on :root, dark ones under prefers-color-scheme.

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
  return [
    "/* Generated from design/tokens.json by scripts/build.mjs. Do not edit. */",
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

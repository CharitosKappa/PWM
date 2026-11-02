// Swift values from design/tokens.json (D41 §5): writes
// apps/apple/PWMKit/Sources/PWMDesign/Tokens.swift. SwiftPM cannot run Node
// at build time, so the file is committed; `--check` (part of `just check`)
// fails when it is stale.
//
//   node scripts/tokens-swift.mjs           write the file
//   node scripts/tokens-swift.mjs --check   fail if it differs
import fs from 'node:fs';
import process from 'node:process';

const SOURCE = 'design/tokens.json';
const OUT = 'apps/apple/PWMKit/Sources/PWMDesign/Tokens.swift';

/** `surface-raised` → `surfaceRaised`; `0` → `s0`. */
function name(key, prefix = '') {
  const camel = key.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());
  return /^[0-9]/.test(camel) ? `${prefix}${camel}` : camel;
}

function rgba(hex) {
  const m = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(hex);
  if (!m) throw new Error(`tokens.json: not a #RRGGBB(AA) color: ${hex}`);
  const n = (i) => `0x${m[1].slice(i, i + 2).toUpperCase()}`;
  const alpha = m[2] === undefined ? '1' : String(Math.round((parseInt(m[2], 16) / 255) * 1000) / 1000);
  return `RGBA(${n(0)}, ${n(2)}, ${n(4)}, alpha: ${alpha})`;
}

function px(value) {
  if (value?.unit !== 'px') throw new Error(`tokens.json: not a px dimension: ${JSON.stringify(value)}`);
  return String(value.value);
}

const entries = (group) => Object.entries(group).filter(([key]) => !key.startsWith('$'));

/** The value a `{font.weight.x}` or `{font.family.x}` reference points at. */
function resolve(tokens, ref) {
  const path = /^\{(.+)\}$/.exec(ref)?.[1];
  if (path === undefined) throw new Error(`tokens.json: not a reference: ${ref}`);
  const token = path.split('.').reduce((node, key) => node?.[key], tokens);
  if (token?.$value === undefined) throw new Error(`tokens.json: unknown reference ${ref}`);
  return { path, value: token.$value };
}

export function tokensSwift(tokens) {
  if (tokens.version?.$value !== 1) throw new Error('tokens.json: unknown version');
  const { light, dark } = tokens.color;
  const colors = entries(light).map(([key, token]) => {
    if (dark[key] === undefined) throw new Error(`tokens.json: no dark value for ${key}`);
    return `        public static let ${name(key)} = ThemedColor(light: ${rgba(token.$value)}, dark: ${rgba(dark[key].$value)})`;
  });
  const types = entries(tokens.type).map(([key, token]) => {
    const v = token.$value;
    const family = resolve(tokens, v.fontFamily).path.split('.').at(-1);
    const weight = resolve(tokens, v.fontWeight).value;
    return `        public static let ${name(key)} = TypeStyle(size: ${px(v.fontSize)}, weight: ${String(weight)}, lineHeight: ${String(v.lineHeight)}, monospaced: ${String(family === 'mono')})`;
  });
  const dims = (group, prefix) =>
    entries(group).map(([key, token]) => `        public static let ${name(key, prefix)}: Double = ${px(token.$value)}`);
  return [
    '// Generated from design/tokens.json by scripts/tokens-swift.mjs. Do not edit.',
    '',
    '/// The shared design tokens (D41 §5). Placeholder values until D1.',
    'public enum Tokens {',
    `    public static let version = ${String(tokens.version.$value)}`,
    '',
    '    public enum Color {',
    ...colors,
    '    }',
    '',
    '    /// Sizes are the web sizes in points; SwiftUI scales them with Dynamic Type.',
    '    public enum Text {',
    ...types,
    '    }',
    '',
    '    public enum Space {',
    ...dims(tokens.space, 's'),
    '    }',
    '',
    '    public enum Radius {',
    ...dims(tokens.radius, 'r'),
    '    }',
    '}',
    '',
  ].join('\n');
}

const swift = tokensSwift(JSON.parse(fs.readFileSync(SOURCE, 'utf8')));
if (process.argv.includes('--check')) {
  const current = fs.existsSync(OUT) ? fs.readFileSync(OUT, 'utf8') : '';
  if (current !== swift) {
    console.error(`${OUT} is stale: run \`node scripts/tokens-swift.mjs\``);
    process.exit(1);
  }
} else {
  fs.mkdirSync(OUT.slice(0, OUT.lastIndexOf('/')), { recursive: true });
  fs.writeFileSync(OUT, swift);
  console.log(`wrote ${OUT}`);
}

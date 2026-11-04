// Kotlin values from design/tokens.json (D52 §6): writes the Android app's
// Tokens.kt, plain values with no Compose types, so `:kit` stays plain JVM.
// The app turns them into Compose colors and text styles. Committed, like
// the Swift file; `--check` (part of `just check`) fails when it is stale.
//
//   node scripts/tokens-kotlin.mjs           write the file
//   node scripts/tokens-kotlin.mjs --check   fail if it differs
import fs from 'node:fs';
import process from 'node:process';

const SOURCE = 'design/tokens.json';
const OUT = 'apps/android/kit/src/main/kotlin/pwm/kit/design/Tokens.kt';

/** `surface-raised` → `surfaceRaised`; `0` → `s0`. */
function name(key, prefix = '') {
  const camel = key.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());
  return /^[0-9]/.test(camel) ? `${prefix}${camel}` : camel;
}

/** `#RRGGBB(AA)` → `0xAARRGGBB`, as Android colors are written. */
function argb(hex) {
  const m = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(hex);
  if (!m) throw new Error(`tokens.json: not a #RRGGBB(AA) color: ${hex}`);
  return `0x${(m[2] ?? 'FF').toUpperCase()}${m[1].toUpperCase()}`;
}

function px(value) {
  if (value?.unit !== 'px') throw new Error(`tokens.json: not a px dimension: ${JSON.stringify(value)}`);
  return String(value.value);
}

const entries = (group) => Object.entries(group).filter(([key]) => !key.startsWith('$'));

function resolve(tokens, ref) {
  const path = /^\{(.+)\}$/.exec(ref)?.[1];
  if (path === undefined) throw new Error(`tokens.json: not a reference: ${ref}`);
  const token = path.split('.').reduce((node, key) => node?.[key], tokens);
  if (token?.$value === undefined) throw new Error(`tokens.json: unknown reference ${ref}`);
  return { path, value: token.$value };
}

export function tokensKotlin(tokens) {
  if (tokens.version?.$value !== 1) throw new Error('tokens.json: unknown version');
  const { light, dark } = tokens.color;
  const colors = entries(light).map(([key, token]) => {
    if (dark[key] === undefined) throw new Error(`tokens.json: no dark value for ${key}`);
    return `        val ${name(key)} = ThemedColor(light = ${argb(token.$value)}, dark = ${argb(dark[key].$value)})`;
  });
  const types = entries(tokens.type).map(([key, token]) => {
    const v = token.$value;
    const family = resolve(tokens, v.fontFamily).path.split('.').at(-1);
    const weight = resolve(tokens, v.fontWeight).value;
    return `        val ${name(key)} = TypeStyle(size = ${px(v.fontSize)}.0, weight = ${String(weight)}, lineHeight = ${String(v.lineHeight)}, monospaced = ${String(family === 'mono')})`;
  });
  const dims = (group, prefix) =>
    entries(group).map(([key, token]) => `        const val ${name(key, prefix)}: Double = ${px(token.$value)}.0`);
  return [
    '// Generated from design/tokens.json by scripts/tokens-kotlin.mjs. Do not edit.',
    'package pwm.kit.design',
    '',
    '/** A color for light and dark themes, as `0xAARRGGBB`. */',
    'data class ThemedColor(val light: Long, val dark: Long)',
    '',
    '/** A text style: size in sp, weight 100 to 900, line height as a multiple. */',
    'data class TypeStyle(val size: Double, val weight: Int, val lineHeight: Double, val monospaced: Boolean)',
    '',
    '/** The shared design tokens (D52 §6). Placeholder values until D1. */',
    'object Tokens {',
    `    const val VERSION = ${String(tokens.version.$value)}`,
    '',
    '    object Color {',
    ...colors,
    '    }',
    '',
    '    /** Sizes are the web sizes in sp; Android scales them with the font size setting. */',
    '    object Text {',
    ...types,
    '    }',
    '',
    '    /** In dp. */',
    '    object Space {',
    ...dims(tokens.space, 's'),
    '    }',
    '',
    '    /** In dp. */',
    '    object Radius {',
    ...dims(tokens.radius, 'r'),
    '    }',
    '}',
    '',
  ].join('\n');
}

const kotlin = tokensKotlin(JSON.parse(fs.readFileSync(SOURCE, 'utf8')));
if (process.argv.includes('--check')) {
  const current = fs.existsSync(OUT) ? fs.readFileSync(OUT, 'utf8') : '';
  if (current !== kotlin) {
    console.error(`${OUT} is stale: run \`node scripts/tokens-kotlin.mjs\``);
    process.exit(1);
  }
} else {
  fs.mkdirSync(OUT.slice(0, OUT.lastIndexOf('/')), { recursive: true });
  fs.writeFileSync(OUT, kotlin);
  console.log(`wrote ${OUT}`);
}

// The plain error messages (D52 §6), from design/messages.json, as Kotlin:
// writes the Android app's Messages.kt. Committed; `--check` (part of
// `just check`) fails when it is stale. The Apple app keeps its own list in
// PWMError.swift until it moves to the same file (D52 implementation notes).
//
//   node scripts/messages-kotlin.mjs           write the file
//   node scripts/messages-kotlin.mjs --check   fail if it differs
import fs from 'node:fs';
import process from 'node:process';

const SOURCE = 'design/messages.json';
const OUT = 'apps/android/kit/src/main/kotlin/pwm/kit/Messages.kt';

/** A Kotlin string literal: JSON's escaping, plus `$`. */
const literal = (text) => JSON.stringify(text).replaceAll('$', '\\$');

export function messagesKotlin(doc) {
  if (doc.version !== 1) throw new Error('messages.json: unknown version');
  const codes = Object.keys(doc.messages);
  if (codes.some((c) => !/^[a-z0-9_]+$/.test(c))) throw new Error('messages.json: codes are lowercase snake_case');
  return [
    '// Generated from design/messages.json by scripts/messages-kotlin.mjs. Do not edit.',
    'package pwm.kit',
    '',
    '/** What to show for each of the core\'s error codes; never a value. */',
    'internal object Messages {',
    `    const val FALLBACK = ${literal(doc.fallback)}`,
    '',
    '    val byCode: Map<String, String> = mapOf(',
    ...codes.map((code) => `        ${literal(code)} to ${literal(doc.messages[code])},`),
    '    )',
    '}',
    '',
  ].join('\n');
}

const kotlin = messagesKotlin(JSON.parse(fs.readFileSync(SOURCE, 'utf8')));
if (process.argv.includes('--check')) {
  const current = fs.existsSync(OUT) ? fs.readFileSync(OUT, 'utf8') : '';
  if (current !== kotlin) {
    console.error(`${OUT} is stale: run \`node scripts/messages-kotlin.mjs\``);
    process.exit(1);
  }
} else {
  fs.writeFileSync(OUT, kotlin);
  console.log(`wrote ${OUT}`);
}

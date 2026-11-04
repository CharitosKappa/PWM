// The plain error messages (D52 §6), from design/messages.json, into both
// apps: the Apple app's Messages.swift and the Android app's Messages.kt.
// Neither build can run Node, so both files are committed; `--check` (part
// of `just check`) fails when either is stale.
//
//   node scripts/messages.mjs           write the files
//   node scripts/messages.mjs --check   fail if either differs
import fs from 'node:fs';
import process from 'node:process';

const SOURCE = 'design/messages.json';
const SWIFT = 'apps/apple/PWMKit/Sources/PWMKit/Messages.swift';
const KOTLIN = 'apps/android/kit/src/main/kotlin/pwm/kit/Messages.kt';
const PLATFORMS = ['apple', 'android'];

/** The messages one platform shows: the shared ones, with its own wording. */
export function messagesFor(doc, platform) {
  if (doc.version !== 1) throw new Error('messages.json: unknown version');
  if (typeof doc.fallback !== 'string' || doc.fallback === '') throw new Error('messages.json: no fallback');
  const codes = Object.keys(doc.messages);
  if (codes.some((c) => !/^[a-z0-9_]+$/.test(c))) throw new Error('messages.json: codes are lowercase snake_case');
  const platforms = doc.platforms ?? {};
  for (const [name, own] of Object.entries(platforms)) {
    if (!PLATFORMS.includes(name)) throw new Error(`messages.json: unknown platform ${name}`);
    const extra = Object.keys(own).filter((c) => !(c in doc.messages));
    if (extra.length) throw new Error(`messages.json: ${name} replaces messages that don't exist: ${extra.join(', ')}`);
  }
  for (const text of [doc.fallback, ...codes.map((c) => doc.messages[c]), ...Object.values(platforms).flatMap(Object.values)]) {
    if (typeof text !== 'string' || text === '') throw new Error('messages.json: a message is empty');
  }
  const own = platforms[platform] ?? {};
  return { fallback: doc.fallback, messages: codes.map((code) => [code, own[code] ?? doc.messages[code]]) };
}

/** A Swift string literal: JSON's escaping, with `\u{…}` for control characters. */
const swiftLiteral = (text) => JSON.stringify(text).replace(/\\u([0-9a-f]{4})/gi, '\\u{$1}');

/** A Kotlin string literal: JSON's escaping, plus `$`. */
const kotlinLiteral = (text) => JSON.stringify(text).replaceAll('$', '\\$');

export function messagesSwift(doc) {
  const { fallback, messages } = messagesFor(doc, 'apple');
  return [
    '// Generated from design/messages.json by scripts/messages.mjs. Do not edit.',
    '',
    'extension PWMError {',
    '    /// What to show for a code with no message.',
    `    static let fallback = ${swiftLiteral(fallback)}`,
    '',
    '    /// What to show for each error code; never a value.',
    '    static let messages: [String: String] = [',
    ...messages.map(([code, text]) => `        ${swiftLiteral(code)}: ${swiftLiteral(text)},`),
    '    ]',
    '}',
    '',
  ].join('\n');
}

export function messagesKotlin(doc) {
  const { fallback, messages } = messagesFor(doc, 'android');
  return [
    '// Generated from design/messages.json by scripts/messages.mjs. Do not edit.',
    'package pwm.kit',
    '',
    "/** What to show for each error code; never a value. */",
    'internal object Messages {',
    `    const val FALLBACK = ${kotlinLiteral(fallback)}`,
    '',
    '    val byCode: Map<String, String> = mapOf(',
    ...messages.map(([code, text]) => `        ${kotlinLiteral(code)} to ${kotlinLiteral(text)},`),
    '    )',
    '}',
    '',
  ].join('\n');
}

const doc = JSON.parse(fs.readFileSync(SOURCE, 'utf8'));
const outputs = [
  [SWIFT, messagesSwift(doc)],
  [KOTLIN, messagesKotlin(doc)],
];
if (process.argv.includes('--check')) {
  const stale = outputs.filter(([path, text]) => (fs.existsSync(path) ? fs.readFileSync(path, 'utf8') : '') !== text);
  for (const [path] of stale) console.error(`${path} is stale: run \`node scripts/messages.mjs\``);
  if (stale.length) process.exit(1);
} else {
  for (const [path, text] of outputs) {
    fs.writeFileSync(path, text);
    console.log(`wrote ${path}`);
  }
}

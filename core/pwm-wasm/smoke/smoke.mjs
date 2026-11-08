// Node smoke test of the WASM build (D31), run by `just wasm` and CI:
// the binding surface, randomness from crypto.getRandomValues, zxcvbn's
// timer, and the snapshot round trip. The logic is tested in Rust.
//
//   node core/pwm-wasm/smoke/smoke.mjs target/wasm/node/pwm_wasm.js

import { createRequire } from 'node:module';
import path from 'node:path';

const require = createRequire(import.meta.url);
const core = require(path.resolve(process.argv[2] ?? 'target/wasm/node/pwm_wasm.js'));

function check(ok, what) {
  if (!ok) {
    console.error(`failed: ${what}`);
    process.exit(1);
  }
}

function code(block) {
  try {
    block();
    return 'none';
  } catch (error) {
    return error instanceof Error ? error.code : 'not an Error';
  }
}

const id = core.newId();
check(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(id), 'a UUIDv4 id');
check(core.newId() !== id, 'ids are random');

const password = core.generatePassword(20, true, true, true, true, false);
check(password.length === 20, 'a 20-character password');
// A space, since a few list words contain "-" ("t-shirt").
check(core.generatePassphrase(5, ' ', false, false).split(' ').length === 5, 'a 5-word passphrase');
check(core.passwordScore('password', []) <= 1, 'a weak password scores low');
check(core.passwordScore(password, []) >= 3, 'a generated password scores high');
check(code(() => core.generatePassword(3, true, true, true, true, false)) === 'invalid_item', 'a too-short length is refused');

const client = new core.WasmClient();
check(!client.isUnlocked(), 'a new client is locked');
check(code(() => client.vaultIds()) === 'locked', 'locked before unlock');
check(
  code(() => client.startSignup('a@example.com', 'password', core.newId(), 'extension', 'Browser')) === 'weak_password',
  'a weak master password is refused',
);
check(code(() => client.startSignup('a@example.com', 'x', core.newId(), 'toaster', 'x')) === 'invalid_item', 'an unknown platform is refused');

// Sign-up starts with the keys made in WASM (Argon2id) and a JSON request.
const step = client.startSignup('a@example.com', 'correct horse battery staple 42', core.newId(), 'extension', 'Browser');
check(step.kind === 'send' && step.method === 'POST', 'sign-up sends a request');
check(step.path === '/v1/auth/register/start', 'to register/start');
const body = JSON.parse(new TextDecoder().decode(step.body));
check(typeof body.account_id === 'string' && typeof body.registration_request === 'string', 'with the OPAQUE request');
check(code(() => client.networkFailed()) === 'network', 'a lost connection ends the sign-up');

// The cache survives a snapshot round trip; damaged snapshots are refused.
const snapshot = client.snapshot();
check(new TextDecoder().decode(snapshot.slice(0, 4)) === 'PWMS', 'a PWMS snapshot');
const reloaded = core.WasmClient.load(snapshot);
check(Buffer.compare(Buffer.from(reloaded.snapshot()), Buffer.from(snapshot)) === 0, 'it loads back identical');
check(code(() => core.WasmClient.load(new Uint8Array([1, 2, 3]))) === 'storage', 'a damaged snapshot is refused');

// Autofill (D33): field classification works without unlocking.
const classes = JSON.parse(
  client.classifyFields(
    JSON.stringify([
      { type: 'text', name: 'user', visible: true },
      { type: 'password', visible: true },
    ]),
    'https://example.com/',
  ),
);
check(classes[0].class === 'username' && classes[1].class === 'current-password', 'login fields are classified');
check(code(() => client.autofillMatches('https://example.com/', undefined)) === 'locked', 'matching needs an unlocked vault');
check(code(() => client.setAutofillRules(new Uint8Array([1, 2, 3]), Date.now(), 0)) === 'invalid_rules', 'unsigned rules are refused');

console.log('wasm bindings: ok');

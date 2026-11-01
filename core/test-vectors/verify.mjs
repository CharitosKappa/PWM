// Independent check of the JSON vectors with Node's built-in crypto (no Rust
// code involved). Run: node core/test-vectors/verify.mjs (part of
// `just test-vectors`).
//
// Not covered here: Argon2id (Node has no Argon2; kdf.json is checked by the
// Rust checker and the RFC 9106 known-answer test). The `pw_key` values from
// account.json are taken as input.
//
// HChaCha20 is implemented below because Node only exposes the IETF
// ChaCha20-Poly1305. This is verification tooling, never product code, and it
// is checked against the draft-irtf-cfrg-xchacha-03 vector before use.

import crypto from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const DIR = dirname(fileURLToPath(import.meta.url));
const hex = (s) => Buffer.from(s, "hex");
const load = (name) => {
  const doc = JSON.parse(readFileSync(join(DIR, `${name}.json`), "utf8"));
  if (doc.format_version !== 1) throw new Error(`${name}: unknown format_version`);
  return doc.vectors;
};

let failures = 0;
let checks = 0;
function check(name, condition) {
  checks++;
  if (!condition) {
    failures++;
    console.error(`FAIL ${name}`);
  }
}
const eq = (a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)) === 0;

// --- XChaCha20-Poly1305 ---

function rotl(v, c) {
  return ((v << c) | (v >>> (32 - c))) >>> 0;
}
function quarter(s, a, b, c, d) {
  s[a] = (s[a] + s[b]) >>> 0; s[d] = rotl(s[d] ^ s[a], 16);
  s[c] = (s[c] + s[d]) >>> 0; s[b] = rotl(s[b] ^ s[c], 12);
  s[a] = (s[a] + s[b]) >>> 0; s[d] = rotl(s[d] ^ s[a], 8);
  s[c] = (s[c] + s[d]) >>> 0; s[b] = rotl(s[b] ^ s[c], 7);
}
function hchacha20(key, nonce16) {
  const s = new Uint32Array(16);
  s.set([0x61707865, 0x3320646e, 0x79622d32, 0x6b206574]);
  for (let i = 0; i < 8; i++) s[4 + i] = key.readUInt32LE(4 * i);
  for (let i = 0; i < 4; i++) s[12 + i] = nonce16.readUInt32LE(4 * i);
  for (let i = 0; i < 10; i++) {
    quarter(s, 0, 4, 8, 12); quarter(s, 1, 5, 9, 13); quarter(s, 2, 6, 10, 14); quarter(s, 3, 7, 11, 15);
    quarter(s, 0, 5, 10, 15); quarter(s, 1, 6, 11, 12); quarter(s, 2, 7, 8, 13); quarter(s, 3, 4, 9, 14);
  }
  const out = Buffer.alloc(32);
  [0, 1, 2, 3, 12, 13, 14, 15].forEach((w, i) => out.writeUInt32LE(s[w], 4 * i));
  return out;
}
function ietfParams(key, nonce24) {
  const subkey = hchacha20(key, nonce24.subarray(0, 16));
  const nonce12 = Buffer.concat([Buffer.alloc(4), nonce24.subarray(16, 24)]);
  return [subkey, nonce12];
}
function xchachaEncrypt(key, nonce24, plaintext, aad) {
  const [subkey, nonce12] = ietfParams(key, nonce24);
  const c = crypto.createCipheriv("chacha20-poly1305", subkey, nonce12, { authTagLength: 16 });
  c.setAAD(aad, { plaintextLength: plaintext.length });
  return Buffer.concat([c.update(plaintext), c.final(), c.getAuthTag()]);
}
function xchachaDecrypt(key, nonce24, ctAndTag, aad) {
  const [subkey, nonce12] = ietfParams(key, nonce24);
  const ct = ctAndTag.subarray(0, -16);
  const d = crypto.createDecipheriv("chacha20-poly1305", subkey, nonce12, { authTagLength: 16 });
  d.setAAD(aad, { plaintextLength: ct.length });
  d.setAuthTag(ctAndTag.subarray(-16));
  return Buffer.concat([d.update(ct), d.final()]);
}
/** Opens a v1 envelope `1 || nonce:24 || ct || tag:16`; throws on failure. */
function openEnvelope(key, envelope, aad) {
  if (envelope[0] !== 1 || envelope.length < 41) throw new Error("bad envelope");
  return xchachaDecrypt(key, envelope.subarray(1, 25), envelope.subarray(25), aad);
}

{
  // draft-irtf-cfrg-xchacha-03 A.3.1
  const ct = xchachaEncrypt(
    hex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f"),
    hex("404142434445464748494a4b4c4d4e4f5051525354555657"),
    Buffer.from("Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it."),
    hex("50515253c0c1c2c3c4c5c6c7"),
  );
  check("xchacha20poly1305 draft A.3.1", ct.toString("hex") ===
    "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb731c7f1b0b4aa6440bf3a82f4eda7e39" +
    "ae64c6708c54c216cb96b72e1213b4522f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9" +
    "21f9664c97637da9768812f615c68b13b52ec0875924c1c7987947deafd8780acf49");
}

// --- Encodings from SECURITY.md ---

function labelled(label, ...fields) {
  return Buffer.concat([Buffer.from([label.length]), Buffer.from(label, "ascii"), ...fields]);
}
const u64 = (n) => { const b = Buffer.alloc(8); b.writeBigUInt64BE(BigInt(n)); return b; };
const aadItem = (version, vault, item, revision) => labelled("pwm/item", Buffer.from([version]), vault, item, u64(revision));
const aadVaultKey = (version, vault) => labelled("pwm/vault-key", Buffer.from([version]), vault);
const aadKeyset = (version, account) => labelled("pwm/keyset", Buffer.from([version]), account);
const aadShare = (version, vault, sender, recipient) => labelled("pwm/share", Buffer.from([version]), vault, sender, recipient);

const hkdf = (ikm, salt, info) => Buffer.from(crypto.hkdfSync("sha256", ikm, salt, info, 32));
const xor = (a, b) => Buffer.from(a.map((x, i) => x ^ b[i]));

const PKCS8 = { ed25519: "302e020100300506032b657004220420", x25519: "302e020100300506032b656e04220420" };
const SPKI = { ed25519: "302a300506032b6570032100", x25519: "302a300506032b656e032100" };
const privateKey = (kind, raw) => crypto.createPrivateKey({ key: Buffer.concat([hex(PKCS8[kind]), raw]), format: "der", type: "pkcs8" });
const publicKey = (kind, raw) => crypto.createPublicKey({ key: Buffer.concat([hex(SPKI[kind]), raw]), format: "der", type: "spki" });
const rawPublic = (priv) => crypto.createPublicKey(priv).export({ format: "der", type: "spki" }).subarray(-32);

const SPACES = /[   -   　]/g;
function normalizePassword(s) {
  const n = s.replace(SPACES, " ").normalize("NFC");
  return n.length === 0 || /\p{Cc}/u.test(n) ? null : n;
}

const DATA = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CHECK = DATA + "*~$=U";
function secretKeyText(bytes) {
  const n = BigInt("0x" + bytes.toString("hex"));
  let s = "";
  for (let i = 25; i >= 0; i--) s += DATA[Number((n >> BigInt(5 * i)) & 31n)];
  s += CHECK[Number(n % 37n)];
  return ["A1", s.slice(0, 6), s.slice(6, 12), s.slice(12, 18), s.slice(18, 24), s.slice(24)].join("-");
}

function openKeyset(v) {
  return openEnvelope(hex(v.auk), hex(v.sealed_keyset), aadKeyset(1, hex(v.account_id)));
}

// --- Vector files ---

for (const v of load("aad")) {
  const aad = {
    item: () => aadItem(v.version, hex(v.vault_id), hex(v.item_id), BigInt(v.revision)),
    "vault-key": () => aadVaultKey(v.version, hex(v.vault_id)),
    keyset: () => aadKeyset(v.version, hex(v.account_id)),
    share: () => aadShare(v.version, hex(v.vault_id), hex(v.sender_ed25519_pub), hex(v.recipient_x25519_pub)),
  }[v.context]();
  check(`aad ${v.context}`, eq(aad, hex(v.aad)));
}

for (const v of load("aead")) {
  const aad = aadItem(v.version, hex(v.vault_id), hex(v.item_id), BigInt(v.revision));
  check("aead aad", eq(aad, hex(v.aad)));
  check("aead open", eq(openEnvelope(hex(v.key), hex(v.envelope), aad), hex(v.plaintext)));
}

for (const v of load("hkdf")) {
  check("hkdf", eq(hkdf(hex(v.ikm), hex(v.salt), hex(v.info)), hex(v.okm)));
}

for (const v of load("password")) {
  const out = normalizePassword(hex(v.input).toString("utf8"));
  check(`password ${v.input}`, v.normalized === null ? out === null : out !== null && eq(Buffer.from(out, "utf8"), hex(v.normalized)));
}

for (const v of load("secret_key")) {
  check(`secret_key ${v.bytes}`, secretKeyText(hex(v.bytes)) === v.text);
}

for (const v of load("account")) {
  const account = hex(v.account_id);
  check("account secret key text", secretKeyText(hex(v.secret_key_bytes)) === v.secret_key);
  const skKey = hkdf(hex(v.secret_key_bytes), account, Buffer.from("pwm/sk/v1"));
  check("account sk_key", eq(skKey, hex(v.sk_key)));
  const mixed = xor(hex(v.pw_key), skKey);
  check("account auk", eq(hkdf(mixed, account, Buffer.from("pwm/auk/v1")), hex(v.auk)));
  check("account auth_secret", eq(hkdf(mixed, account, Buffer.from("pwm/auth/v1")), hex(v.auth_secret)));
  const plaintext = openKeyset(v);
  check("account keyset plaintext", eq(plaintext, hex(v.keyset_plaintext)) && plaintext.length === 97 && plaintext[0] === 1);
  check("account x25519_public", eq(rawPublic(privateKey("x25519", plaintext.subarray(33, 65))), hex(v.x25519_public)));
  check("account ed25519_public", eq(rawPublic(privateKey("ed25519", plaintext.subarray(65, 97))), hex(v.ed25519_public)));
}

for (const v of load("vault_key")) {
  const symKey = openKeyset(v).subarray(1, 33);
  check("vault_key sym_key", eq(symKey, hex(v.sym_key)));
  const vaultKey = openEnvelope(symKey, hex(v.wrapped), aadVaultKey(1, hex(v.vault_id)));
  check("vault_key unwrap", eq(vaultKey, hex(v.vault_key)));
  const probe = openEnvelope(vaultKey, hex(v.probe_envelope), aadItem(1, hex(v.vault_id), hex(v.probe_item_id), BigInt(v.probe_revision)));
  check("vault_key probe", eq(probe, hex(v.probe_plaintext)));
}

for (const v of load("signature")) {
  const seed = openKeyset(v).subarray(65, 97);
  check("signature seed", eq(seed, hex(v.ed25519_seed)));
  const msg = labelled(v.label, hex(v.message));
  const sig = crypto.sign(null, msg, privateKey("ed25519", seed));
  check("signature bytes", eq(sig, hex(v.signature)));
  check("signature verify", crypto.verify(null, msg, publicKey("ed25519", hex(v.ed25519_public)), hex(v.signature)));
}

for (const v of load("share")) {
  const share = hex(v.share);
  check("share version", share[0] === 1);
  const ephPub = share.subarray(1, 33);
  const sig = share.subarray(33, 97);
  const envelope = share.subarray(97);
  const vault = hex(v.vault_id);
  const senderPub = hex(v.sender.ed25519_public);
  const recipientPub = hex(v.recipient.x25519_public);
  const recipientSecret = openKeyset(v.recipient).subarray(33, 65);
  check("share recipient secret", eq(recipientSecret, hex(v.recipient.x25519_secret)));
  check("share recipient public", eq(rawPublic(privateKey("x25519", recipientSecret)), recipientPub));
  const msg = labelled("pwm/share-sig", Buffer.from([1]), vault, recipientPub, ephPub, envelope);
  check("share signature", crypto.verify(null, msg, publicKey("ed25519", senderPub), sig));
  const dh = crypto.diffieHellman({ privateKey: privateKey("x25519", recipientSecret), publicKey: publicKey("x25519", ephPub) });
  check("share dh contributory", !eq(dh, Buffer.alloc(32)));
  const shareKey = hkdf(dh, Buffer.concat([ephPub, recipientPub]), Buffer.from("pwm/share/v1"));
  const vaultKey = openEnvelope(shareKey, envelope, aadShare(1, vault, senderPub, recipientPub));
  check("share vault key", eq(vaultKey, hex(v.vault_key)));
}

if (failures > 0) {
  console.error(`${failures} of ${checks} independent vector checks failed`);
  process.exit(1);
}
console.log(`independent vector checks ok (${checks} checks, Node ${process.version})`);

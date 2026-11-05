// Independent check of the JSON vectors with Node's built-in crypto (no Rust
// code involved). Run: node core/test-vectors/verify.mjs (part of
// `just test-vectors`).
//
// Passkeys (passkey.json): every byte layout and the ES256 signature are
// checked here with Node's own P-256.
//
// Not covered here: Argon2id (Node has no Argon2; kdf.json is checked by the
// Rust checker and the RFC 9106 known-answer test). The `pw_key` values from
// account.json and export.json are taken as input. OPAQUE (opaque.json) is
// checked by the Rust checker only.
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
const aadDeviceName = (version, account, device) => labelled("pwm/device-name", Buffer.from([version]), account, device);
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

// Export files (D24): parse the .pwmx header, then HKDF and the envelope from pw_key.
const u32 = (n) => { const b = Buffer.alloc(4); b.writeUInt32BE(n); return b; };
for (const v of load("export")) {
  const file = hex(v.file);
  const p = v.params;
  check("export magic and version", file.subarray(0, 5).toString("latin1") === "PWMX\x01");
  check("export header params", file.readUInt32BE(5) === p.memory_kib && file.readUInt32BE(9) === p.iterations && file.readUInt32BE(13) === p.parallelism);
  const salt = file.subarray(17, 33);
  check("export header salt", eq(salt, hex(v.salt)));
  const aad = labelled("pwm/export", Buffer.from([1]), u32(p.memory_kib), u32(p.iterations), u32(p.parallelism), salt);
  check("export aad", eq(aad, hex(v.aad)));
  const exportKey = hkdf(hex(v.pw_key), salt, Buffer.from("pwm/export/v1"));
  check("export key", eq(exportKey, hex(v.export_key)));
  check("export open", eq(openEnvelope(exportKey, file.subarray(33), aad), hex(v.plaintext)));
}

// TOTP (D23): parse the stored value independently, then RFC 4226 truncation.
const B32 = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
function fromBase32(s) {
  let bits = 0, value = 0;
  const out = [];
  for (const c of s.toUpperCase().replace(/[ =]/g, "")) {
    const v = B32.indexOf(c);
    if (v < 0) throw new Error("bad base32");
    value = (value << 5) | v; bits += 5;
    if (bits >= 8) { bits -= 8; out.push((value >>> bits) & 255); }
  }
  return Buffer.from(out);
}
function otpValue(alg, key, counter) {
  const c = Buffer.alloc(8); c.writeBigUInt64BE(BigInt(counter));
  const h = crypto.createHmac(alg, key).update(c).digest();
  return h.readUInt32BE(h[h.length - 1] & 15) & 0x7fffffff;
}
function totpConfig(input) {
  if (/^steam:\/\//i.test(input)) return { key: fromBase32(input.slice(8)), alg: "sha1", digits: 0, period: 30 };
  if (!/^otpauth:\/\//i.test(input)) return { key: fromBase32(input), alg: "sha1", digits: 6, period: 30 };
  const url = new URL(input);
  const p = Object.fromEntries([...url.searchParams].map(([k, v]) => [k.toLowerCase(), v]));
  if (p.encoder?.toLowerCase() === "steam") return { key: fromBase32(p.secret), alg: "sha1", digits: 0, period: 30 };
  return { key: fromBase32(p.secret), alg: (p.algorithm ?? "SHA1").toLowerCase(), digits: Number(p.digits ?? 6), period: Number(p.period ?? 30) };
}
const STEAM = "23456789BCDFGHJKMNPQRTVWXY";
for (const v of load("totp")) {
  if (v.kind === "hotp") {
    check(`hotp ${v.counter}`, String(otpValue("sha1", hex(v.key), v.counter) % 1e6).padStart(6, "0") === v.code);
    continue;
  }
  const t = totpConfig(v.uri);
  let value = otpValue(t.alg, t.key, Math.floor(v.time / t.period));
  let code = "";
  if (t.digits === 0) {
    for (let i = 0; i < 5; i++) { code += STEAM[value % 26]; value = Math.floor(value / 26); }
  } else {
    code = String(value % 10 ** t.digits).padStart(t.digits, "0");
  }
  check(`${v.kind} ${v.time}`, (v.kind === "steam") === (t.digits === 0) && code === v.code);
}

for (const v of load("device_name")) {
  const symKey = openKeyset(v).subarray(1, 33);
  check("device_name sym_key", eq(symKey, hex(v.sym_key)));
  const aad = aadDeviceName(1, hex(v.account_id), hex(v.device_id));
  check("device_name aad", eq(aad, hex(v.aad)));
  const name = hex(v.name);
  const padded = Buffer.concat([name, Buffer.from([0x80]), Buffer.alloc(63 - name.length)]);
  check("device_name padding", padded.length === 64 && eq(padded, hex(v.padded)));
  check("device_name open", eq(openEnvelope(symKey, hex(v.envelope), aad), padded));
}

// Device approval (D46): both sides' X25519, the commitment, the code and the envelope.
for (const v of load("approval")) {
  const id = hex(v.approval_id);
  const nPub = hex(v.new_public);
  const ePub = hex(v.existing_public);
  check("approval new public", eq(rawPublic(privateKey("x25519", hex(v.new_secret))), nPub));
  check("approval existing public", eq(rawPublic(privateKey("x25519", hex(v.existing_secret))), ePub));
  const label = Buffer.from("pwm/approval-commit");
  const commitment = crypto.createHash("sha256").update(Buffer.concat([Buffer.from([label.length]), label, nPub])).digest();
  check("approval commitment", eq(commitment, hex(v.commitment)));
  const dhNew = crypto.diffieHellman({ privateKey: privateKey("x25519", hex(v.new_secret)), publicKey: publicKey("x25519", ePub) });
  const dhExisting = crypto.diffieHellman({ privateKey: privateKey("x25519", hex(v.existing_secret)), publicKey: publicKey("x25519", nPub) });
  check("approval dh", eq(dhNew, dhExisting) && !eq(dhNew, Buffer.alloc(32)));
  const salt = Buffer.concat([id, nPub, ePub]);
  const key = hkdf(dhNew, salt, Buffer.from("pwm/approval/v1"));
  const code = hkdf(dhNew, salt, Buffer.from("pwm/approval-code/v1")).readUInt32BE(0) % 1000000;
  check("approval code", code === v.code);
  const digits = String(code).padStart(6, "0");
  check("approval code text", `${digits.slice(0, 3)} ${digits.slice(3)}` === v.code_text);
  const aad = labelled("pwm/approval", Buffer.from([1]), id, nPub, ePub);
  check("approval aad", eq(aad, hex(v.aad)));
  check("approval open", openEnvelope(key, hex(v.envelope), aad).toString("utf8") === v.secret_key);
}

for (const v of load("biometric_unlock")) {
  const keyset = openKeyset(v);
  check("biometric keyset", eq(keyset, hex(v.keyset_plaintext)));
  const aad = labelled("pwm/biometric-unlock", Buffer.from([1]), hex(v.account_id), hex(v.device_id));
  check("biometric aad", eq(aad, hex(v.aad)));
  // The envelope; pwm-client prefixes it with v (= 1) || account_id.
  check("biometric open", eq(openEnvelope(hex(v.unlock_key), hex(v.envelope), aad), keyset));
}

for (const v of load("browser_unlock")) {
  const keyset = openKeyset(v);
  check("browser unlock keyset", eq(keyset, hex(v.keyset_plaintext)));
  const aad = labelled("pwm/browser-unlock", Buffer.from([1]), hex(v.account_id), hex(v.device_id));
  check("browser unlock aad", eq(aad, hex(v.aad)));
  // The envelope; pwm-client prefixes it with v (= 1) || account_id.
  check("browser unlock open", eq(openEnvelope(hex(v.unlock_key), hex(v.envelope), aad), keyset));
}

for (const v of load("session")) {
  const account = hex(v.account_id);
  const keyset = openKeyset(v);
  check("session keyset", eq(keyset, hex(v.keyset_plaintext)));
  const sessionAad = labelled("pwm/session", Buffer.from([1]), account);
  check("session aad", eq(sessionAad, hex(v.session_aad)));
  // The envelope; pwm-client prefixes it with v (= 1) || account_id.
  check("session open", eq(openEnvelope(hex(v.session_key), hex(v.session_blob), sessionAad), keyset));
  const symKey = keyset.subarray(1, 33);
  check("session sym_key", eq(symKey, hex(v.sym_key)));
  const tokensAad = labelled("pwm/device-tokens", Buffer.from([1]), account);
  check("device_tokens aad", eq(tokensAad, hex(v.device_tokens_aad)));
  check(
    "device_tokens open",
    eq(openEnvelope(symKey, hex(v.device_secret_envelope), tokensAad), hex(v.device_secret)),
  );
}

const b64u = (t) => Buffer.from(t, "base64url");

for (const v of load("passkey")) {
  // Registration: authenticatorData layout, the COSE key, the "none" attestation.
  const reg = v.registration.response;
  const auth = b64u(reg.authenticatorData);
  const rpId = JSON.parse(v.create_request).rp.id;
  check("passkey rp id hash", eq(auth.subarray(0, 32), crypto.createHash("sha256").update(rpId).digest()));
  check("passkey create flags", auth[32] === 0x5d);
  check("passkey sign count 0", auth.readUInt32BE(33) === 0);
  const idLen = auth.readUInt16BE(53);
  check("passkey credential id", eq(auth.subarray(55, 55 + idLen), hex(v.credential_id)));
  const cose = auth.subarray(55 + idLen);
  check("passkey cose header", eq(cose.subarray(0, 10), hex("a5010203262001215820")));
  const x = cose.subarray(10, 42);
  const y = cose.subarray(45, 77);
  const fromCose = crypto.createPublicKey({ key: { kty: "EC", crv: "P-256", x: x.toString("base64url"), y: y.toString("base64url") }, format: "jwk" });
  const spki = crypto.createPublicKey({ key: b64u(reg.publicKey), format: "der", type: "spki" });
  check("passkey spki matches cose", eq(spki.export({ format: "der", type: "spki" }), fromCose.export({ format: "der", type: "spki" })));
  const priv = crypto.createPrivateKey({ key: hex(v.private_key_pkcs8), format: "der", type: "pkcs8" });
  check("passkey private key matches", eq(crypto.createPublicKey(priv).export({ format: "der", type: "spki" }), b64u(reg.publicKey)));
  const att = b64u(reg.attestationObject);
  const head = Buffer.concat([Buffer.from([0xa3, 0x63]), Buffer.from("fmt"), Buffer.from([0x64]), Buffer.from("none"), Buffer.from([0x67]), Buffer.from("attStmt"), Buffer.from([0xa0, 0x68]), Buffer.from("authData"), Buffer.from([0x58, auth.length])]);
  check("passkey attestation object", eq(att, Buffer.concat([head, auth])));
  const created = JSON.parse(b64u(reg.clientDataJSON).toString("utf8"));
  check("passkey create client data", created.type === "webauthn.create" && created.origin === v.origin && created.challenge === JSON.parse(v.create_request).challenge && created.crossOrigin === false);

  // Assertion: the signature over authenticatorData || SHA-256(clientDataJSON).
  const a = v.assertion.response;
  const aAuth = b64u(a.authenticatorData);
  check("passkey get flags", aAuth.length === 37 && aAuth[32] === 0x1d);
  const clientData = b64u(a.clientDataJSON);
  const signed = Buffer.concat([aAuth, crypto.createHash("sha256").update(clientData).digest()]);
  check("passkey signature", crypto.verify("sha256", signed, { key: spki, dsaEncoding: "der" }, b64u(a.signature)));
  check("passkey user handle", a.userHandle === JSON.parse(v.create_request).user.id);
  const got = JSON.parse(clientData.toString("utf8"));
  check("passkey get client data", got.type === "webauthn.get" && got.origin === v.origin);
}

for (const v of load("passkey_hashed")) {
  // A platform hands over the client data hash (D47): the same layouts, and
  // the signature over authenticatorData || that hash.
  const att = hex(v.attestation_object);
  const head = Buffer.concat([Buffer.from([0xa3, 0x63]), Buffer.from("fmt"), Buffer.from([0x64]), Buffer.from("none"), Buffer.from([0x67]), Buffer.from("attStmt"), Buffer.from([0xa0, 0x68]), Buffer.from("authData"), Buffer.from([0x58])]);
  check("hashed attestation head", eq(att.subarray(0, head.length), head));
  const authData = att.subarray(head.length + 1);
  check("hashed attestation length", att[head.length] === authData.length);
  check("hashed rp id hash", eq(authData.subarray(0, 32), crypto.createHash("sha256").update(v.rp_id).digest()));
  check("hashed create flags", authData[32] === 0x5d);
  const idLen = authData.readUInt16BE(53);
  check("hashed credential id", eq(authData.subarray(55, 55 + idLen), hex(v.credential_id)));
  const cose = authData.subarray(55 + idLen);
  const x = cose.subarray(10, 42);
  const y = cose.subarray(45, 77);
  const pub = crypto.createPublicKey({ key: { kty: "EC", crv: "P-256", x: x.toString("base64url"), y: y.toString("base64url") }, format: "jwk" });
  const priv = crypto.createPrivateKey({ key: hex(v.private_key_pkcs8), format: "der", type: "pkcs8" });
  check("hashed private key matches", eq(crypto.createPublicKey(priv).export({ format: "der", type: "spki" }), pub.export({ format: "der", type: "spki" })));
  const aAuth = hex(v.authenticator_data);
  check("hashed get flags", aAuth.length === 37 && aAuth[32] === 0x1d && eq(aAuth.subarray(0, 32), authData.subarray(0, 32)));
  const signed = Buffer.concat([aAuth, hex(v.client_data_hash)]);
  check("hashed signature", crypto.verify("sha256", signed, { key: pub, dsaEncoding: "der" }, hex(v.signature)));
}

for (const v of load("passkey_android")) {
  // Android's Credential Manager (D61): an app's client data is built by the
  // provider; a privileged browser's is not, and only its hash is signed.
  const reg = v.registration.response;
  const auth = b64u(reg.authenticatorData);
  check(`android ${v.name} rp id hash`, eq(auth.subarray(0, 32), crypto.createHash("sha256").update("example.com").digest()));
  check(`android ${v.name} create flags`, auth[32] === 0x5d && auth.readUInt32BE(33) === 0);
  const pub = crypto.createPublicKey({ key: b64u(reg.publicKey), format: "der", type: "spki" });
  const priv = crypto.createPrivateKey({ key: hex(v.private_key_pkcs8), format: "der", type: "pkcs8" });
  check(`android ${v.name} private key matches`, eq(crypto.createPublicKey(priv).export({ format: "der", type: "spki" }), b64u(reg.publicKey)));
  const a = v.assertion.response;
  const aAuth = b64u(a.authenticatorData);
  check(`android ${v.name} get flags`, aAuth.length === 37 && aAuth[32] === 0x1d);
  let hash;
  if (v.name === "app") {
    const origin = "android:apk-key-hash:" + hex(v.cert_sha256).toString("base64url");
    const expect = (type, request) => `{"type":"${type}","challenge":"${JSON.parse(request).challenge}","origin":"${origin}","androidPackageName":"${v.package}"}`;
    check("android app create client data", b64u(reg.clientDataJSON).toString("utf8") === expect("webauthn.create", v.create_request));
    const got = b64u(a.clientDataJSON);
    check("android app get client data", got.toString("utf8") === expect("webauthn.get", v.get_request));
    hash = crypto.createHash("sha256").update(got).digest();
  } else {
    check("android browser placeholders", reg.clientDataJSON === "e30" && a.clientDataJSON === "e30");
    hash = hex(v.client_data_hash);
  }
  check(`android ${v.name} signature`, crypto.verify("sha256", Buffer.concat([aAuth, hash]), { key: pub, dsaEncoding: "der" }, b64u(a.signature)));
}

// bitwarden_export.json: Bitwarden's password-protected export (D51 §4). The
// keys are derived again and both values are opened, MAC first.
for (const v of load("bitwarden_export")) {
  const e = v.export;
  const key = crypto.pbkdf2Sync(v.password, e.salt, e.kdfIterations, 32, "sha256");
  const expand = (info) => crypto.createHmac("sha256", key).update(Buffer.concat([Buffer.from(info), Buffer.from([1])])).digest();
  const enc = expand("enc");
  const mac = expand("mac");
  check(`bitwarden ${v.name} keys`, eq(enc, hex(v.enc_key)) && eq(mac, hex(v.mac_key)));
  const open = (s) => {
    const [iv, ct, tag] = s.slice(2).split("|").map((p) => Buffer.from(p, "base64"));
    const want = crypto.createHmac("sha256", mac).update(Buffer.concat([iv, ct])).digest();
    if (!eq(want, tag)) return null;
    const d = crypto.createDecipheriv("aes-256-cbc", enc, iv);
    return Buffer.concat([d.update(ct), d.final()]);
  };
  check(`bitwarden ${v.name} validation`, open(e.encKeyValidation_DO_NOT_EDIT) !== null);
  check(`bitwarden ${v.name} data`, open(e.data)?.toString("utf8") === v.plaintext);
}

// opaque.json is checked by the Rust checker only: the protocol itself is
// covered by the RFC 9807 vectors in opaque-ke.

if (failures > 0) {
  console.error(`${failures} of ${checks} independent vector checks failed`);
  process.exit(1);
}
console.log(`independent vector checks ok (${checks} checks, Node ${process.version})`);

// Writes bitwarden_export.json: Bitwarden's password-protected export (D51
// §4) made with Node's built-in crypto, independently of the Rust importer
// that reads it. Run once: node core/test-vectors/bitwarden_export.mjs.
// The randomness is fixed, so a rerun gives the same file.
//
// The format, from Bitwarden's published description:
// - key = PBKDF2-SHA256(password, salt, iterations, 32)
// - enc = HKDF-Expand-SHA256(key, "enc", 32), mac = HKDF-Expand-SHA256(key, "mac", 32)
// - each value is "2." + b64(iv) + "|" + b64(AES-256-CBC(enc, iv, data)) + "|" + b64(HMAC-SHA256(mac, iv || ct))

import crypto from "node:crypto";
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const DIR = dirname(fileURLToPath(import.meta.url));

// HKDF-Expand only (the key is already uniform): T(1) = HMAC(prk, info || 0x01).
const expand = (prk, info) =>
  crypto.createHmac("sha256", prk).update(Buffer.concat([Buffer.from(info), Buffer.from([1])])).digest();

function encString(enc, mac, iv, plaintext) {
  const cipher = crypto.createCipheriv("aes-256-cbc", enc, iv);
  const ct = Buffer.concat([cipher.update(plaintext), cipher.final()]);
  const tag = crypto.createHmac("sha256", mac).update(Buffer.concat([iv, ct])).digest();
  return `2.${iv.toString("base64")}|${ct.toString("base64")}|${tag.toString("base64")}`;
}

const password = "correct horse battery staple 42";
const salt = "c2FsdHlzYWx0eXNhbHR5";
const iterations = 5000;
const inner = {
  encrypted: false,
  folders: [{ id: "f1", name: "Work/Banks" }],
  items: [
    {
      id: "i1",
      folderId: "f1",
      type: 1,
      name: "Bank",
      notes: "synthetic",
      favorite: true,
      login: {
        username: "ada",
        password: "hunter2-synthetic",
        totp: "JBSWY3DPEHPK3PXP",
        uris: [{ match: 1, uri: "https://bank.example" }],
      },
    },
  ],
};
const key = crypto.pbkdf2Sync(password, salt, iterations, 32, "sha256");
const enc = expand(key, "enc");
const mac = expand(key, "mac");
const iv1 = Buffer.alloc(16, 1);
const iv2 = Buffer.alloc(16, 2);
const exportFile = {
  encrypted: true,
  passwordProtected: true,
  salt,
  kdfType: 0,
  kdfIterations: iterations,
  kdfMemory: null,
  kdfParallelism: null,
  encKeyValidation_DO_NOT_EDIT: encString(enc, mac, iv1, Buffer.from("b3a7c2e0-5f1d-4b8e-9c0a-1d2e3f405162")),
  data: encString(enc, mac, iv2, Buffer.from(JSON.stringify(inner))),
};
const doc = {
  format_version: 1,
  description:
    "Bitwarden password-protected export (D51 §4), made with Node's crypto. `plaintext` is the inner JSON the export holds.",
  vectors: [
    {
      name: "pbkdf2 5000",
      password,
      enc_key: enc.toString("hex"),
      mac_key: mac.toString("hex"),
      export: exportFile,
      plaintext: JSON.stringify(inner),
    },
  ],
};
writeFileSync(join(DIR, "bitwarden_export.json"), `${JSON.stringify(doc, null, 2)}\n`);
console.log("wrote bitwarden_export.json");

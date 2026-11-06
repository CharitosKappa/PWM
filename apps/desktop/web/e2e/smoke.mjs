// The desktop app end to end (D66 §9), through tauri-driver's WebDriver
// API with plain fetch: create an account in the real window, save the
// Secret Key, lock, see a wrong password refused, unlock, copy a
// generated password, sign out. CI runs it on Linux under Xvfb, with the
// app's settings pointing at a local server.
//
//   node e2e/smoke.mjs <path to the built app>
//
// tauri-driver must be listening on 127.0.0.1:4444.

import assert from "node:assert/strict";
import console from "node:console";
import process from "node:process";
import { setTimeout } from "node:timers";

/* global fetch -- Node 22's */

const DRIVER = "http://127.0.0.1:4444";
const app = process.argv[2];
if (!app) throw new Error("usage: smoke.mjs <app>");

async function call(method, path, body) {
  const response = await fetch(DRIVER + path, {
    method,
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await response.json();
  if (!response.ok)
    throw new Error(`${method} ${path}: ${JSON.stringify(json)}`);
  return json.value;
}

const session = await call("POST", "/session", {
  capabilities: { alwaysMatch: { "tauri:options": { application: app } } },
});
const id = session.sessionId;

/** Runs `script` (a function body) in the window with `args`. */
const run = (script, ...args) =>
  call("POST", `/session/${id}/execute/sync`, { script, args });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Waits until the window's text contains `text`. */
async function waitFor(text, ms = 30_000) {
  const end = Date.now() + ms;
  for (;;) {
    const body = await run("return document.body.innerText;");
    if (body.includes(text)) return body;
    if (Date.now() > end) throw new Error(`no "${text}" in: ${body}`);
    await sleep(250);
  }
}

const click = (text) =>
  run(
    `const b = [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === arguments[0] && !b.disabled);
     if (!b) throw new Error("no enabled button " + arguments[0]);
     b.click();`,
    text,
  );

/** Types into the field labelled `label` (or the first password field). */
const type = (label, value) =>
  run(
    `const input = arguments[0] === "*password"
       ? document.querySelector("input[type=password]")
       : [...document.querySelectorAll("label")].find((l) => l.querySelector(".label")?.textContent.trim() === arguments[0])?.querySelector("input");
     if (!input) throw new Error("no field " + arguments[0]);
     Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, arguments[1]);
     input.dispatchEvent(new Event("input", { bubbles: true }));`,
    label,
    value,
  );

const PASSWORD = "tangerine orbit velvet canyon 42";
const email = `e2e-${Date.now()}@example.com`;

try {
  await waitFor("Create account");
  await click("Create account");
  await type("Email", email);
  await type("Master password", PASSWORD);
  await type("Confirm master password", PASSWORD);
  await waitFor("Very strong");
  await run(`document.querySelector("button.primary").click();`);
  await waitFor("Save your Secret Key");
  await click("Copy");
  await waitFor("The clipboard clears in 30 seconds.");
  await click("I saved it");
  const vault = await waitFor("No items yet");
  assert.ok(vault.includes(email), vault);

  await click("Lock");
  await waitFor("Unlock");
  await type("*password", "not the password at all");
  await sleep(300);
  await run(`document.querySelector("button.primary").click();`);
  await waitFor("Wrong master password.");
  await type("*password", PASSWORD);
  await sleep(300);
  await run(`document.querySelector("button.primary").click();`);
  await waitFor("No items yet");
  await waitFor("Synced");

  await click("Generate");
  await waitFor("Passphrase");
  await click("Copy");

  await click("Settings");
  await waitFor("Sign out of this device");
  // Browser-only settings never show on the desktop.
  const settings = await run("return document.body.innerText;");
  assert.ok(!settings.includes("Fill and save on sign-in pages"), settings);
  assert.ok(!settings.includes("passkeys"), settings);
  await click("Sign out of this device");
  await waitFor("Create account");
  console.log("smoke: ok");
} finally {
  await call("DELETE", `/session/${id}`).catch(() => undefined);
}

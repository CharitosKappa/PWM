// The fixture suite, data-driven (D39 §1): every page in fixtures/ has an
// entry in fixtures/fixtures.json saying what must happen there, and this
// runs each one against the extension's test build in Chromium.
import { expect, test } from "@playwright/test";
import type { FrameLocator, Locator, Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { Harness } from "../lib/extension.ts";

interface Fixture {
  page: string;
  scheme?: "https" | "http";
  expect: "fill" | "refuse" | "confirm" | "copy" | "no_form" | "flow";
  reason?: string;
  frames?: string[];
  ready?: string;
  wait?: number;
  values?: Record<string, "username" | "password" | "code">;
  empty?: string[];
  text?: Record<string, "username">;
  test?: string;
}

const DIR = path.resolve(import.meta.dirname, "../fixtures");
const manifest = JSON.parse(
  fs.readFileSync(path.join(DIR, "fixtures.json"), "utf8"),
) as { fixtures: Fixture[] };

const USERNAME = "ada@site-a.test";
const PASSWORD = "Site-A secret 7!";

let harness: Harness;
let item = "";

test.beforeAll(async () => {
  harness = await Harness.start({ hostRules: "MAP *.test 127.0.0.1" });
  item = await harness.saveLogin({
    title: "Site A",
    username: USERNAME,
    password: PASSWORD,
    url: "https://site-a.test:8443",
    totp: "otpauth://totp/Site%20A:ada?secret=JBSWY3DPEHPK3PXP&issuer=Site%20A",
  });
});

test.afterAll(async () => {
  await harness.close();
});

test("every fixture page has an entry", () => {
  const pages = fs.readdirSync(DIR).filter((f) => f.endsWith(".html"));
  const listed = new Set(manifest.fixtures.map((f) => f.page));
  expect(pages.filter((p) => !listed.has(p))).toEqual([]);
  expect([...listed].filter((p) => !pages.includes(p))).toEqual([]);
});

function scope(page: Page, frames: string[] = []): Page | FrameLocator {
  let at: Page | FrameLocator = page;
  for (const selector of frames) at = at.frameLocator(selector);
  return at;
}

function field(page: Page, fixture: Fixture, selector: string): Locator {
  return scope(page, fixture.frames).locator(selector);
}

const expected = {
  username: USERNAME,
  password: PASSWORD,
  code: /^\d{6}$/,
};

async function filledAsExpected(page: Page, fixture: Fixture): Promise<void> {
  for (const [selector, kind] of Object.entries(fixture.values ?? {})) {
    await expect(field(page, fixture, selector)).toHaveValue(expected[kind]);
  }
  for (const [selector, kind] of Object.entries(fixture.text ?? {})) {
    await expect(field(page, fixture, selector)).toHaveText(expected[kind]);
  }
}

async function untouched(page: Page, fixture: Fixture): Promise<void> {
  for (const selector of fixture.empty ?? []) {
    await expect(field(page, fixture, selector)).toHaveValue("");
  }
}

for (const fixture of manifest.fixtures.filter((f) => f.expect !== "flow")) {
  const scheme = fixture.scheme ?? "https";
  const port = scheme === "https" ? 8443 : 8081;
  const url = `${scheme}://site-a.test:${String(port)}/${fixture.page}`;
  const name = `${fixture.page}${fixture.scheme === "http" ? " (http)" : ""}: ${fixture.expect}${fixture.reason ? ` (${fixture.reason})` : ""}`;

  test(name, async () => {
    const page = await harness.context.newPage();
    await page.goto(url);
    if (fixture.ready !== undefined) {
      await field(page, fixture, fixture.ready).waitFor();
    }
    if (fixture.wait !== undefined) await page.waitForTimeout(fixture.wait);
    const outcome = await harness.fill(page, item);
    switch (fixture.expect) {
      case "fill":
        expect(outcome.code).toBe("ok");
        await filledAsExpected(page, fixture);
        break;
      case "confirm":
        expect(outcome.code).toBe(fixture.reason);
        await untouched(page, {
          ...fixture,
          empty: Object.keys(fixture.values ?? {}),
        });
        expect((await harness.fill(page, item, true)).code).toBe("ok");
        await filledAsExpected(page, fixture);
        break;
      case "refuse":
        expect(outcome.code).toBe(fixture.reason);
        break;
      case "copy":
        expect(outcome).toEqual({ code: "ok", copiedCode: true });
        break;
      case "no_form":
        expect(outcome.code).toBe("no_form");
        break;
    }
    await untouched(page, fixture);
    await page.close();
  });
}

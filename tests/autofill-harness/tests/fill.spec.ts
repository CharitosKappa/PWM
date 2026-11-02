// Filling on user action with the built extension in Chromium (D35 §8):
// what must fill, warn or refuse. The start of the M4.7 harness.
import { chromium, expect, test } from "@playwright/test";
import type { BrowserContext, Frame, Page, Worker } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const EXTENSION = path.resolve(
  import.meta.dirname,
  "../../../extension/dist-test",
);
const SERVER = process.env.PWM_SERVER ?? "http://127.0.0.1:8080";
const A = "https://site-a.test:8443";
const PASSWORD = "Site-A secret 7!";

let context: BrowserContext;
let worker: Worker;
let vaultId = "";
const ids: Record<string, string> = {};

/** What the test build exposes in the service worker (`build.mjs --test`). */
interface Background {
  handle(request: object): Promise<unknown>;
  saveItem(json: string): Promise<{ vaultId: string; itemId: string }>;
  autofill: {
    fillTab(
      tabId: number,
      vaultId: string,
      itemId: string,
      confirmed: boolean,
    ): Promise<unknown>;
  };
}

declare global {
  var pwmTest: { ready: Promise<Background> };
  // Only what the service worker code below uses.
  var chrome: {
    tabs: { query(query: { url: string }): Promise<{ id?: number }[]> };
  };
}

/** Runs `fn` in the extension's service worker, next to its background. */
function inBackground<T, A>(fn: (arg: A) => Promise<T>, arg: A): Promise<T> {
  return worker.evaluate(fn as (arg: unknown) => Promise<T>, arg);
}

test.beforeAll(async () => {
  if (!fs.existsSync(path.join(EXTENSION, "manifest.json"))) {
    throw new Error(
      "no test build: pnpm --filter @pwm/extension build:test (after just wasm)",
    );
  }
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), "pwm-harness-"));
  context = await chromium.launchPersistentContext(profile, {
    channel: "chromium",
    headless: true,
    ignoreHTTPSErrors: true,
    args: [
      `--disable-extensions-except=${EXTENSION}`,
      `--load-extension=${EXTENSION}`,
      "--host-resolver-rules=MAP *.test 127.0.0.1",
      "--ignore-certificate-errors",
    ],
  });
  worker =
    context.serviceWorkers()[0] ??
    (await context.waitForEvent("serviceworker"));

  // A fresh account on the local server, with a few logins.
  const created = await inBackground(
    async ({ server, email, password, items }) => {
      const b = await globalThis.pwmTest.ready;
      await b.handle({
        kind: "setSettings",
        settings: { serverUrl: server, autoLockMinutes: 60 },
      });
      await b.handle({ kind: "signUp", email, password });
      const out: Record<string, string> = {};
      let vault = "";
      for (const [key, item] of Object.entries(items)) {
        const now = Date.now();
        const saved = await b.saveItem(
          JSON.stringify({
            id: crypto.randomUUID(),
            v: 1,
            type: "login",
            title: item.title,
            created_at: now,
            updated_at: now,
            login: {
              username: item.username,
              password: item.password,
              urls: [{ url: item.url }],
              totp: item.totp,
            },
          }),
        );
        out[key] = saved.itemId;
        vault = saved.vaultId;
      }
      return { ids: out, vault };
    },
    {
      server: SERVER,
      email: `harness-${String(Date.now())}@example.com`,
      password: "correct horse battery staple umbrella",
      items: {
        siteA: {
          title: "Site A",
          username: "ada@site-a.test",
          password: PASSWORD,
          url: A,
          totp: "otpauth://totp/Site%20A:ada?secret=JBSWY3DPEHPK3PXP&issuer=Site%20A",
        },
      },
    },
  );
  Object.assign(ids, created.ids);
  vaultId = created.vault;
});

test.afterAll(async () => {
  await context.close();
});

/** What the shortcut or the popup's Fill does, on the tab showing `page`. */
async function fill(page: Page, confirmed = false): Promise<string> {
  return inBackground(
    async ({ url, vault, item, confirmed }) => {
      const b = await globalThis.pwmTest.ready;
      const [tab] = await chrome.tabs.query({ url });
      if (tab?.id === undefined) return "no tab";
      try {
        await b.autofill.fillTab(tab.id, vault, item, confirmed);
        return "ok";
      } catch (error) {
        return (error as { code?: string }).code ?? String(error);
      }
    },
    { url: page.url(), vault: vaultId, item: ids.siteA ?? "", confirmed },
  );
}

async function open(url: string): Promise<Page> {
  const page = await context.newPage();
  await page.goto(url);
  return page;
}

test("the popup lists the login with its username (the core's JSON)", async () => {
  const items = await inBackground(async () => {
    const b = await globalThis.pwmTest.ready;
    return b.handle({ kind: "items", query: "" });
  }, null);
  expect(items).toEqual([
    expect.objectContaining({
      title: "Site A",
      username: "ada@site-a.test",
      hasPassword: true,
      hasCode: true,
    }),
  ]);
});

test("fills a plain login form", async () => {
  const page = await open(`${A}/plain.html`);
  expect(await fill(page)).toBe("ok");
  await expect(page.locator("#email")).toHaveValue("ada@site-a.test");
  await expect(page.locator("#password")).toHaveValue(PASSWORD);
  await page.close();
});

test("never fills honeypot fields", async () => {
  const page = await open(`${A}/honeypot.html`);
  expect(await fill(page)).toBe("ok");
  await expect(page.locator("#email")).toHaveValue("ada@site-a.test");
  await expect(page.locator("#password")).toHaveValue(PASSWORD);
  await expect(page.locator("#trap-hidden")).toHaveValue("");
  await expect(page.locator("#trap-offscreen")).toHaveValue("");
  await page.close();
});

test("fills a multi-step login one page at a time", async () => {
  const page = await open(`${A}/multistep1.html`);
  expect(await fill(page)).toBe("ok");
  await expect(page.locator("#email")).toHaveValue("ada@site-a.test");
  await page.goto(`${A}/multistep2.html`);
  expect(await fill(page)).toBe("ok");
  await expect(page.locator("#password")).toHaveValue(PASSWORD);
  await page.close();
});

test("fills a one-time code field", async () => {
  const page = await open(`${A}/code.html`);
  expect(await fill(page)).toBe("ok");
  await expect(page.locator("#code")).toHaveValue(/^\d{6}$/);
  await page.close();
});

test("refuses a field under a transparent overlay", async () => {
  const page = await open(`${A}/overlay.html`);
  expect(await fill(page)).toBe("field_hidden");
  await expect(page.locator("#email")).toHaveValue("");
  await expect(page.locator("#password")).toHaveValue("");
  await page.close();
});

test("refuses a field with a nearly transparent ancestor", async () => {
  const page = await open(`${A}/opacity.html`);
  expect(await fill(page)).toBe("field_hidden");
  await expect(page.locator("#password")).toHaveValue("");
  await page.close();
});

test("refuses a login form in a frame from another site", async () => {
  const page = await open(`${A}/frame-cross.html`);
  expect(await fill(page)).toBe("refused");
  const frame = page.frameLocator("iframe");
  await expect(frame.locator("#password")).toHaveValue("");
  await page.close();
});

test("fills a login form in a frame from the same site", async () => {
  const page = await open(`${A}/frame-same.html`);
  expect(await fill(page)).toBe("ok");
  const frame = page.frameLocator("iframe");
  await expect(frame.locator("#password")).toHaveValue(PASSWORD);
  await page.close();
});

test("asks before filling on an http page", async () => {
  const page = await open("http://site-a.test:8081/plain.html");
  expect(await fill(page)).toBe("needs_confirm");
  await expect(page.locator("#password")).toHaveValue("");
  expect(await fill(page, true)).toBe("ok");
  await expect(page.locator("#password")).toHaveValue(PASSWORD);
  await page.close();
});

test.describe("the inline icon", () => {
  test.beforeAll(async () => {
    const on = await inBackground(async () => {
      const b = await globalThis.pwmTest.ready;
      return b.handle({ kind: "setInlineIcon", on: true });
    }, null);
    expect(on).toBe(true);
  });

  async function iconFrame(page: Page): Promise<Frame> {
    await expect
      .poll(() => page.frames().some((f) => f.url().includes("inline.html")))
      .toBe(true);
    const frame = page.frames().find((f) => f.url().includes("inline.html"));
    if (!frame) throw new Error("no icon frame");
    await frame.locator("button.icon").waitFor();
    return frame;
  }

  test("fills after two checked clicks: the icon, then the login", async () => {
    const page = await open(`${A}/plain.html`);
    const frame = await iconFrame(page);
    // The frame takes clicks only after 500 ms of being visible.
    await page.waitForTimeout(800);
    await frame.locator("button.icon").click();
    await expect(frame.locator("body")).toHaveClass("menu");
    await page.waitForTimeout(800);
    await frame.locator("button.item", { hasText: "Site A" }).click();
    await expect(page.locator("#password")).toHaveValue(PASSWORD);
    await expect(page.locator("#email")).toHaveValue("ada@site-a.test");
    await page.close();
  });

  test("ignores clicks on an icon the page made transparent", async () => {
    const page = await open(`${A}/attack-icon.html`);
    const frame = await iconFrame(page);
    await page.waitForTimeout(800);
    await frame.locator("button.icon").click({ force: true });
    await page.waitForTimeout(800);
    await expect(frame.locator("body")).toHaveClass("icon");
    await expect(page.locator("#password")).toHaveValue("");
    await page.close();
  });
});

// Flows with the built extension in Chromium: the inline icon (D35), save
// and update offers (D36), passkeys (D37) and failure reports (D38). The
// single-page fill and refuse cases are data-driven in fixtures.spec.ts.
import { expect, test } from "@playwright/test";
import type { BrowserContext, Frame, Page, Worker } from "@playwright/test";
import { Harness } from "../lib/extension.ts";

const A = "https://site-a.test:8443";
const PASSWORD = "Site-A secret 7!";

let harness: Harness;
let context: BrowserContext;
let worker: Worker;
const ids: Record<string, string> = {};

/** Runs `fn` in the extension's service worker, next to its background. */
function inBackground<T, A>(fn: (arg: A) => Promise<T>, arg: A): Promise<T> {
  return worker.evaluate(fn as (arg: unknown) => Promise<T>, arg);
}

test.beforeAll(async () => {
  harness = await Harness.start({ hostRules: "MAP *.test 127.0.0.1" });
  context = harness.context;
  worker = harness.worker;
  ids.siteA = await harness.saveLogin({
    title: "Site A",
    username: "ada@site-a.test",
    password: PASSWORD,
    url: A,
    totp: "otpauth://totp/Site%20A:ada?secret=JBSWY3DPEHPK3PXP&issuer=Site%20A",
  });
});

test.afterAll(async () => {
  await harness.close();
});

/** What the shortcut or the popup's Fill does: "ok" or the error code. */
async function fill(
  page: Page,
  confirmed = false,
  item = ids.siteA ?? "",
): Promise<string> {
  return (await harness.fill(page, item, confirmed)).code;
}

/** Console output and errors per page, for failure messages. */
const logs = new WeakMap<Page, string[]>();

async function open(url: string): Promise<Page> {
  const page = await context.newPage();
  const lines: string[] = [];
  logs.set(page, lines);
  page.on("console", (m) => lines.push(`${m.type()}: ${m.text()}`));
  page.on("pageerror", (e) => lines.push(`error: ${e.message}`));
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

test("fills a multi-step login one page at a time", async () => {
  const page = await open(`${A}/multistep1.html`);
  expect(await fill(page)).toBe("ok");
  await expect(page.locator("#email")).toHaveValue("ada@site-a.test");
  await page.goto(`${A}/multistep2.html`);
  expect(await fill(page)).toBe("ok");
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
    try {
      await expect
        .poll(() => page.frames().some((f) => f.url().includes("inline.html")))
        .toBe(true);
    } catch (error) {
      // Say what was there instead, so a failure in CI explains itself.
      const state = await inBackground(async () => {
        const b = await globalThis.pwmTest.ready;
        return {
          inline: await b.autofill.inlineIcon(),
          scripts: await chrome.scripting.getRegisteredContentScripts(),
          url: chrome.runtime.getURL("inline.html"),
        };
      }, null);
      const found = {
        frames: page.frames().map((f) => f.url()),
        hosts: await page.locator("pwm-inline").count(),
        logs: logs.get(page) ?? [],
        ...state,
      };
      throw new Error(`no icon frame: ${JSON.stringify(found)}`, {
        cause: error,
      });
    }
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
    try {
      await expect(page.locator("#password")).toHaveValue(PASSWORD);
    } catch (error) {
      // The menu says why it did not fill.
      const says = await frame.locator("body").innerText();
      throw new Error(`not filled; the menu says: ${says}`, { cause: error });
    }
    await expect(page.locator("#email")).toHaveValue("ada@site-a.test");
    await page.close();
  });

  test("ignores clicks on an icon covered after it appears", async () => {
    const page = await open(`${A}/icon-covered-later.html`);
    const frame = await iconFrame(page);
    await expect(page.locator("#cover")).toBeAttached();
    await page.waitForTimeout(800);
    // The cover lets the click through, but the icon is not visible.
    await frame.locator("button.icon").click({ force: true });
    await page.waitForTimeout(800);
    await expect(frame.locator("body")).toHaveClass("icon");
    await expect(page.locator("#password")).toHaveValue("");
    await page.close();
  });

  test("ignores a click on an icon moved under the cursor", async () => {
    const page = await open(`${A}/icon-moved.html`);
    const frame = await iconFrame(page);
    await page.waitForTimeout(800);
    const decoy = await page.locator("#decoy").boundingBox();
    if (!decoy) throw new Error("no decoy");
    const x = decoy.x + decoy.width / 2;
    const y = decoy.y + decoy.height / 2;
    // Reaching the decoy makes the page slide the icon under the pointer;
    // the click comes right after.
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.up();
    await page.waitForTimeout(800);
    await expect(frame.locator("body")).toHaveClass("icon");
    await expect(page.locator("#password")).toHaveValue("");
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

interface SavedLogin {
  id: string;
  title: string;
  username: string;
  password: string;
  history: string[];
  passkeys: { rp_id: string; user_name: string }[];
}

/** Every login in the vault, with old passwords (via the core, test only). */
function logins(): Promise<SavedLogin[]> {
  return inBackground(async () => {
    const b = await globalThis.pwmTest.ready;
    const out: SavedLogin[] = [];
    for (const vault of b.core.vaultIds()) {
      for (const json of b.core.items(vault)) {
        const item = JSON.parse(json) as {
          id: string;
          type: string;
          title: string;
          login?: {
            username: string;
            password: string;
            passkeys?: { rp_id: string; user_name: string }[];
          };
          history?: { login?: { password: string } }[];
        };
        if (item.type !== "login" || !item.login) continue;
        out.push({
          id: item.id,
          title: item.title,
          username: item.login.username,
          password: item.login.password,
          history: (item.history ?? []).map((h) => h.login?.password ?? ""),
          passkeys: (item.login.passkeys ?? []).map((k) => ({
            rp_id: k.rp_id,
            user_name: k.user_name,
          })),
        });
      }
    }
    return out;
  }, null);
}

test.describe("save offers", () => {
  const C = "https://site-c.test:8443";

  test.beforeAll(async () => {
    const on = await inBackground(async () => {
      const b = await globalThis.pwmTest.ready;
      return b.handle({ kind: "setInlineIcon", on: true });
    }, null);
    expect(on).toBe(true);
  });

  /** The save card's frame, once it shows. */
  async function card(page: Page): Promise<Frame> {
    let found: Frame | undefined;
    await expect
      .poll(
        async () => {
          for (const frame of page.frames()) {
            if (!frame.url().includes("inline.html")) continue;
            if ((await frame.locator("body.offer").count()) > 0) found = frame;
          }
          return found !== undefined;
        },
        { timeout: 10_000 },
      )
      .toBe(true);
    if (!found) throw new Error("no card");
    await found.locator("button").first().waitFor();
    return found;
  }

  async function noCard(page: Page): Promise<void> {
    // Long enough for a navigation or a form going away to count.
    await page.waitForTimeout(3000);
    for (const frame of page.frames()) {
      if (!frame.url().includes("inline.html")) continue;
      // Quote the card, so a failure says what it offered.
      const says =
        (await frame.locator("body.offer").count()) > 0
          ? await frame.locator("body").innerText()
          : "";
      expect(says).toBe("");
    }
  }

  async function signIn(
    page: Page,
    url: string,
    email: string,
    password: string,
  ): Promise<void> {
    await page.goto(url);
    await page.fill("#email", email);
    await page.fill("#password", password);
    await page.click("#submit");
  }

  test("offers to save after a sign-in that worked, and the login fills next time", async () => {
    const page = await context.newPage();
    await signIn(
      page,
      `${C}/save-login.html`,
      "carol@site-c.test",
      "carol-secret",
    );
    await page.waitForURL(/save-done/);
    const frame = await card(page);
    await expect(frame.locator("body")).toContainText(
      "Save the login for site-c.test?",
    );
    await expect(frame.locator("body")).not.toContainText("carol-secret");
    await page.waitForTimeout(800);
    await frame.locator("button.primary", { hasText: "Save" }).click();
    await expect
      .poll(async () =>
        (await logins()).find((l) => l.username === "carol@site-c.test"),
      )
      .toMatchObject({ title: "site-c.test", password: "carol-secret" });

    const saved = (await logins()).find(
      (l) => l.username === "carol@site-c.test",
    );
    await page.goto(`${C}/plain.html`);
    expect(await fill(page, false, saved?.id ?? "")).toBe("ok");
    await expect(page.locator("#password")).toHaveValue("carol-secret");
    await page.close();
  });

  test("no offer after a failed sign-in", async () => {
    const page = await context.newPage();
    await signIn(page, `${C}/save-failed.html`, "carl@site-c.test", "wrong");
    await noCard(page);
    expect(
      (await logins()).some((l) => l.username === "carl@site-c.test"),
    ).toBe(false);
    await page.close();
  });

  test("no offer for a login saved as it is", async () => {
    const page = await context.newPage();
    await signIn(
      page,
      `${C}/save-login.html`,
      "carol@site-c.test",
      "carol-secret",
    );
    await page.waitForURL(/save-done/);
    await noCard(page);
    await page.close();
  });

  test("offers to save after a single-page sign-in without a form submit", async () => {
    const page = await context.newPage();
    await page.goto(`${C}/save-spa.html`);
    await page.fill("#email", "spa@site-c.test");
    await page.fill("#password", "spa-secret");
    await page.click("#submit");
    const frame = await card(page);
    await page.waitForTimeout(800);
    await frame.locator("button.primary", { hasText: "Save" }).click();
    await expect
      .poll(async () =>
        (await logins()).find((l) => l.username === "spa@site-c.test"),
      )
      .toMatchObject({ password: "spa-secret" });
    await page.close();
  });

  test("ignores clicks on a card the page made transparent", async () => {
    const page = await context.newPage();
    await signIn(
      page,
      "https://site-d.test:8443/attack-card-login.html",
      "dave@site-d.test",
      "dave-secret",
    );
    await page.waitForURL(/attack-card-done/);
    const frame = await card(page);
    await page.waitForTimeout(800);
    await frame.locator("button.primary").click({ force: true });
    await page.waitForTimeout(1000);
    expect(
      (await logins()).some((l) => l.username === "dave@site-d.test"),
    ).toBe(false);
    await page.close();
  });

  test("saves from the popup's Save login from this page", async () => {
    const page = await context.newPage();
    await page.goto("https://site-e.test:8443/plain.html");
    await page.fill("#email", "eve@site-e.test");
    await page.fill("#password", "eve-secret");
    await page.bringToFront();
    const view = await inBackground(async () => {
      const b = await globalThis.pwmTest.ready;
      return b.handle({ kind: "captureNow" });
    }, null);
    expect(view).toMatchObject({ action: "save", username: "eve@site-e.test" });
    await inBackground(async () => {
      const b = await globalThis.pwmTest.ready;
      return b.handle({ kind: "acceptOffer", itemId: null });
    }, null);
    expect(
      (await logins()).find((l) => l.username === "eve@site-e.test"),
    ).toMatchObject({
      password: "eve-secret",
    });
    await page.close();
  });

  // Last: it changes Site A's password, which the fill tests use.
  test("a change-password form offers an update, and history keeps the old one", async () => {
    const page = await context.newPage();
    await page.goto(`${A}/change-password.html`);
    await page.fill("#current", PASSWORD);
    await page.fill("#new", "Site-A new 8!");
    await page.fill("#confirm", "Site-A new 8!");
    await page.click("#submit");
    await page.waitForURL(/save-done/);
    const frame = await card(page);
    await expect(frame.locator("body")).toContainText("Update the password");
    await page.waitForTimeout(800);
    await frame.locator("button.primary", { hasText: "Update" }).click();
    await expect
      .poll(async () => (await logins()).find((l) => l.id === ids.siteA))
      .toMatchObject({ password: "Site-A new 8!", history: [PASSWORD] });
    await page.close();
  });
});

declare global {
  // The fixture relying party (fixtures/passkey-rp.html).
  var register: (rpId?: string) => Promise<string>;
  var signIn: () => Promise<string>;
  var rp: { aaguid?: string };
}

test.describe("passkeys", () => {
  const P = "https://site-p.test:8443/passkey-rp.html";
  // PWM's AAGUID (pwm-passkey), base64url.
  const AAGUID = Buffer.from(
    "6c9e3d418a274f0b9d52e17730c458a6",
    "hex",
  ).toString("base64url");

  test.beforeAll(async () => {
    const on = await inBackground(async () => {
      const b = await globalThis.pwmTest.ready;
      return b.handle({ kind: "setPasskeys", on: true });
    }, null);
    expect(on).toBe(true);
  });

  const passkeyWindow = () =>
    context.waitForEvent("page", {
      predicate: (p) => p.url().includes("passkey.html"),
      timeout: 15_000,
    });

  test("registers a passkey and signs in with it, after confirming in PWM's window", async () => {
    const page = await context.newPage();
    await page.goto(P);

    const opened = passkeyWindow();
    const registered = page.evaluate(() => window.register());
    const create = await opened;
    await expect(create.locator("body")).toContainText("Create a passkey?");
    await expect(create.locator("body")).toContainText("pat on site-p.test");
    await create
      .locator("button.primary", { hasText: "Create passkey" })
      .click();
    expect(await registered).toBe("registered");
    expect(await page.evaluate(() => window.rp.aaguid)).toBe(AAGUID);
    expect(
      (await logins()).find((l) => l.title === "site-p.test"),
    ).toMatchObject({
      username: "pat",
      passkeys: [{ rp_id: "site-p.test", user_name: "pat" }],
    });

    const again = passkeyWindow();
    const signedIn = page.evaluate(() => window.signIn());
    const pick = await again;
    await pick.locator("button.item", { hasText: "pat" }).click();
    expect(await signedIn).toBe("signed in as pat-1");
    await page.close();
  });

  test("refuses an RP id of another site, without a window", async () => {
    const page = await context.newPage();
    await page.goto(P);
    const before = context.pages().length;
    expect(await page.evaluate(() => window.register("evil.test"))).toBe(
      "SecurityError",
    );
    expect(context.pages().length).toBe(before);
    await page.close();
  });

  test("closing the window cancels the sign-in", async () => {
    const page = await context.newPage();
    await page.goto(P);
    // This page has not registered yet; give it the vault's passkey's key.
    const opened = passkeyWindow();
    const registered = page.evaluate(() => window.register());
    await (await opened).locator("button.primary").click();
    expect(await registered).toBe("registered");

    const again = passkeyWindow();
    const signedIn = page.evaluate(() => window.signIn());
    await (await again).close();
    expect(await signedIn).toBe("NotAllowedError");
    await page.close();
  });

  test("Use another device or the browser hands the request to the browser", async () => {
    const page = await context.newPage();
    // The browser's own WebAuthn, answered by a virtual authenticator.
    const cdp = await context.newCDPSession(page);
    await cdp.send("WebAuthn.enable");
    await cdp.send("WebAuthn.addVirtualAuthenticator", {
      options: {
        protocol: "ctap2",
        transport: "internal",
        hasResidentKey: true,
        hasUserVerification: true,
        isUserVerified: true,
        automaticPresenceSimulation: true,
      },
    });
    await page.goto(P);
    const opened = passkeyWindow();
    const registered = page.evaluate(() => window.register());
    await (
      await opened
    )
      .locator("button", { hasText: "Use another device or the browser" })
      .click();
    expect(await registered).toBe("registered");
    // Made by the browser's authenticator, not by PWM.
    expect(await page.evaluate(() => window.rp.aaguid)).not.toBe(AAGUID);
    await page.close();
  });
});

test.describe("failure reports", () => {
  test("a report holds the form's structure, with personal text and typed values left out", async () => {
    const page = await context.newPage();
    await page.goto(
      "https://site-r.test:8443/report-personal.html?session=abc123#top",
    );
    await page.fill("#email", "typed-user@example.com");
    await page.fill("#password", "typed-secret");
    await page.bringToFront();
    const preview = (await inBackground(async () => {
      const b = await globalThis.pwmTest.ready;
      return b.handle({ kind: "reportPreview", problem: "nothing_filled" });
    }, null)) as { json: string; fields: number };
    const report = JSON.parse(preview.json) as {
      site: string;
      frames: { fields: { label: string; class: string }[] }[];
    };
    expect(report.site).toBe("https://site-r.test:8443");
    expect(preview.fields).toBe(2);
    expect(report.frames[0]?.fields.map((f) => f.label)).toEqual([
      "hello ada (<email>), sign in",
      "password for account #",
    ]);
    expect(report.frames[0]?.fields.map((f) => f.class)).toEqual([
      "username",
      "current-password",
    ]);
    for (const absent of [
      "ada@example.com",
      "4242424242",
      "typed-user",
      "typed-secret",
      "session",
      "abc123",
      "report-personal",
      "Welcome back",
    ]) {
      expect(preview.json).not.toContain(absent);
    }
    await page.close();
  });
});

// The manage page (D44) with the built extension and the local sync
// server: the health report names a weak login, this device is renamed on
// the server, and a JSON export downloads with the vault in it. The leak
// check is not run here: it would reach the real HIBP. Then the items
// (M4.14, D67): a password shown only after Show, an edit, the trash and
// back, the popup refused, and a lock that reloads the page. One account
// for all of it: the server allows 5 sign-ups an hour per address.
import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import fs from "node:fs";
import { Harness } from "../lib/extension.ts";

const MASTER = "correct horse battery staple umbrella";
const FORUM_PASSWORD = "forum-pw-5d1c9e";

let harness: Harness;
let page: Page;
let id: string;

test.beforeAll(async () => {
  harness = await Harness.start();
  await harness.saveLogin({
    title: "Weak site",
    username: "ada",
    password: "password1",
    url: "https://weak.example/login",
  });
  await harness.saveLogin({
    title: "Forum",
    username: "ada@example.com",
    password: FORUM_PASSWORD,
    url: "https://forum.example.com/login",
  });
  id = new URL(harness.worker.url()).host;
  page = await harness.context.newPage();
  await page.goto(`chrome-extension://${id}/manage.html`);
});

test.afterAll(async () => {
  await harness.close();
});

test("the health report names the weak login and links to its site", async () => {
  await page.getByRole("tab", { name: "Password health" }).click();
  const weak = page.locator("section", {
    has: page.getByRole("heading", { name: "Weak" }),
  });
  await expect(weak.getByText("Weak site")).toBeVisible();
  await expect(weak.getByRole("link", { name: "Change" })).toHaveAttribute(
    "href",
    "https://weak.example/login",
  );
});

test("this device is renamed on the server", async () => {
  await page.getByRole("tab", { name: "Devices" }).click();
  const current = page.locator("#device-current");
  await expect(current).toContainText("Browser extension");
  await expect(current.getByRole("button", { name: "Sign out" })).toHaveCount(
    0,
  );
  await current.getByRole("button", { name: "Rename" }).click();
  await current.getByLabel("Name").fill("Harness Chromium");
  await current.getByRole("button", { name: "Save" }).click();
  await expect(current).toContainText("Harness Chromium");
  // Read back from the server, not the page.
  await page.reload();
  await page.getByRole("tab", { name: "Devices" }).click();
  await expect(page.locator("#device-current")).toContainText(
    "Harness Chromium",
  );
});

test("a JSON export asks for the master password and downloads the vault", async () => {
  await page.getByRole("tab", { name: "Export" }).click();
  await page.locator("#export-format").selectOption("json");
  await page.getByLabel("Master password").fill("not the password");
  await page.locator("#export-understood").check();
  await page.locator("#export").click();
  await expect(page.getByRole("alert")).toContainText("Wrong master password");

  await page.getByLabel("Master password").fill(MASTER);
  await page.locator("#export-understood").check();
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.locator("#export").click(),
  ]);
  expect(download.suggestedFilename()).toMatch(
    /^PWM export \d{4}-\d{2}-\d{2}\.json$/,
  );
  const file = await download.path();
  const text = fs.readFileSync(file, "utf8");
  expect(text).toContain("Weak site");
  expect(text).toContain("password1");
  await expect(page.getByLabel("Master password")).toHaveValue("");
});

const passwordRow = () =>
  page.locator(".detail-row", {
    has: page.getByText("Password", { exact: true }),
  });

test("the Items tab shows a password only after Show", async () => {
  await page.getByRole("tab", { name: "Items" }).click();
  await page.locator(".row-button", { hasText: "Forum" }).click();
  await expect(page.getByText("ada@example.com").last()).toBeVisible();
  await expect(page.locator("body")).not.toContainText(FORUM_PASSWORD);
  await passwordRow().getByRole("button", { name: "Show" }).click();
  await expect(passwordRow()).toContainText(FORUM_PASSWORD);
});

test("an edit saves", async () => {
  await page.getByRole("button", { name: "Edit" }).click();
  await page.getByLabel("Title").fill("Forum (work)");
  await page.locator("form.item-editor button.primary").click();
  await expect(page.getByText("Saved.")).toBeVisible();
  await expect(
    page.locator(".row-button", { hasText: "Forum (work)" }),
  ).toBeVisible();
  // Read back from the background, not the page.
  const titles = await harness.inBackground(async () => {
    const b = await globalThis.pwmTest.ready;
    const vault = (await b.handle({ kind: "vault" })) as {
      items: { title: string }[];
    };
    return vault.items.map((i) => i.title);
  }, null);
  expect(titles).toContain("Forum (work)");
});

test("a delete goes to the trash, and Restore brings it back", async () => {
  await page.getByRole("button", { name: "Delete" }).click();
  await expect(page.getByText("Moved to the trash.")).toBeVisible();
  await page.getByRole("button", { name: "Trash", exact: true }).click();
  await page.locator(".row-button", { hasText: "Forum (work)" }).click();
  await page.getByRole("button", { name: "Restore" }).click();
  await expect(page.getByText("Restored.")).toBeVisible();
});

test("health, devices and export are tabs of the same page", async () => {
  await page
    .getByRole("button", { name: "Password health, devices and export" })
    .click();
  await expect(
    page.getByRole("tab", { name: "Password health" }),
  ).toHaveAttribute("aria-selected", "true");
  await page.getByRole("tab", { name: "Items" }).click();
  await expect(
    page.locator(".row-button", { hasText: "Forum (work)" }),
  ).toBeVisible();
});

test("the popup can't ask for a hidden value", async () => {
  const popup = await harness.context.newPage();
  await popup.goto(`chrome-extension://${id}/popup.html`);
  const reply = await popup.evaluate(
    async ({ vaultId }) => {
      const runtime = (
        globalThis as unknown as {
          chrome: { runtime: { sendMessage(m: object): Promise<unknown> } };
        }
      ).chrome.runtime;
      return runtime.sendMessage({
        target: "background",
        request: { kind: "itemForEdit", vaultId, itemId: "x" },
      });
    },
    { vaultId: harness.vaultId },
  );
  expect(reply).toEqual({ ok: false, error: "unsupported" });
  await popup.close();
});

test("a lock reloads the page, so nothing revealed stays", async () => {
  await page.locator(".row-button", { hasText: "Forum (work)" }).click();
  await passwordRow().getByRole("button", { name: "Show" }).click();
  await expect(passwordRow()).toContainText(FORUM_PASSWORD);
  await harness.inBackground(async () => {
    const b = await globalThis.pwmTest.ready;
    await b.handle({ kind: "lock" });
  }, null);
  await expect(page.getByRole("heading", { name: "Unlock" })).toBeVisible();
  await expect(page.locator("body")).not.toContainText(FORUM_PASSWORD);
});

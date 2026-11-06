// Items on the extension's manage page (M4.14, D67) with the built
// extension and the local sync server: the detail shows a password only
// after Show, an edit saves, a delete goes to the trash and back, the popup
// can't ask for a hidden value, and a lock reloads the page.
import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { Harness } from "../lib/extension.ts";

const PASSWORD = "forum-pw-5d1c9e";

let harness: Harness;
let page: Page;
let id: string;

test.beforeAll(async () => {
  harness = await Harness.start();
  await harness.saveLogin({
    title: "Forum",
    username: "ada@example.com",
    password: PASSWORD,
    url: "https://forum.example.com/login",
  });
  id = new URL(harness.worker.url()).host;
  page = await harness.context.newPage();
  await page.goto(`chrome-extension://${id}/manage.html#items`);
});

test.afterAll(async () => {
  await harness.close();
});

const passwordRow = () =>
  page.locator(".detail-row", {
    has: page.getByText("Password", { exact: true }),
  });

test("the detail shows the password only after Show", async () => {
  await page.locator(".row-button", { hasText: "Forum" }).click();
  await expect(page.getByText("ada@example.com").last()).toBeVisible();
  await expect(page.locator("body")).not.toContainText(PASSWORD);
  await passwordRow().getByRole("button", { name: "Show" }).click();
  await expect(passwordRow()).toContainText(PASSWORD);
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
  expect(titles).toEqual(["Forum (work)"]);
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
  await expect(passwordRow()).toContainText(PASSWORD);
  await harness.inBackground(async () => {
    const b = await globalThis.pwmTest.ready;
    await b.handle({ kind: "lock" });
  }, null);
  await expect(page.getByRole("heading", { name: "Unlock" })).toBeVisible();
  await expect(page.locator("body")).not.toContainText(PASSWORD);
});

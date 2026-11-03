// The manage page (D44) with the built extension and the local sync
// server: the health report names a weak login, this device is renamed on
// the server, and a JSON export downloads with the vault in it. The leak
// check is not run here: it would reach the real HIBP.
import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import fs from "node:fs";
import { Harness } from "../lib/extension.ts";

const MASTER = "correct horse battery staple umbrella";

let harness: Harness;
let page: Page;

test.beforeAll(async () => {
  harness = await Harness.start();
  await harness.saveLogin({
    title: "Weak site",
    username: "ada",
    password: "password1",
    url: "https://weak.example/login",
  });
  const id = new URL(harness.worker.url()).host;
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

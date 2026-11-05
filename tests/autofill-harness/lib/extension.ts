// The extension's test build in Chromium, with an account on the local
// sync server (D35 §8, D39). Shared by the browser tests and the top-sites
// runner. The test build exposes the background on `globalThis.pwmTest`.
import { chromium } from "@playwright/test";
import type { BrowserContext, Page, Worker } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export const EXTENSION = path.resolve(
  import.meta.dirname,
  "../../../extension/dist-test",
);
export const SERVER = process.env.PWM_SERVER ?? "http://127.0.0.1:8080";

/** What the test build exposes in the service worker (`build.mjs --test`). */
export interface Background {
  handle(request: object): Promise<unknown>;
  /** Private in TypeScript; the tests read items' history through it. */
  core: { vaultIds(): string[]; items(vaultId: string): string[] };
  saveItem(json: string): Promise<{ vaultId: string; itemId: string }>;
  autofill: {
    inlineIcon(): Promise<boolean>;
    fillTab(
      tabId: number,
      vaultId: string,
      itemId: string,
      confirmed: boolean,
    ): Promise<{ copiedCode: boolean }>;
  };
}

declare global {
  var pwmTest: { ready: Promise<Background> };
  // Only what the service worker code in the tests uses.
  var chrome: {
    tabs: {
      query(query: {
        url?: string;
        active?: boolean;
        lastFocusedWindow?: boolean;
      }): Promise<{ id?: number; url?: string }[]>;
    };
    scripting: { getRegisteredContentScripts(): Promise<unknown[]> };
    runtime: { getURL(path: string): string };
  };
}

export interface LoginSpec {
  title: string;
  username: string;
  password: string;
  url: string;
  totp?: string;
}

/** What a fill gave: "ok" or the background's error code. */
export interface FillOutcome {
  code: string;
  copiedCode: boolean;
}

export interface LaunchOptions {
  /** Added to Chromium's own user agent (the runner identifies itself). */
  userAgentSuffix?: string;
  /** Map these host patterns to the local fixture server. */
  hostRules?: string;
}

export class Harness {
  // Written out, not as parameter properties: the runner runs under Node's
  // type stripping, which can't remove those (erasableSyntaxOnly).
  readonly context: BrowserContext;
  readonly worker: Worker;
  vaultId: string;

  private constructor(
    context: BrowserContext,
    worker: Worker,
    vaultId: string,
  ) {
    this.context = context;
    this.worker = worker;
    this.vaultId = vaultId;
  }

  /** Launches Chromium with the extension and signs up a fresh account. */
  static async start(options: LaunchOptions = {}): Promise<Harness> {
    if (!fs.existsSync(path.join(EXTENSION, "manifest.json"))) {
      throw new Error(
        "no test build: pnpm --filter @pwm/extension build:test (after just wasm)",
      );
    }
    const args = [
      `--disable-extensions-except=${EXTENSION}`,
      `--load-extension=${EXTENSION}`,
      "--ignore-certificate-errors",
    ];
    if (options.hostRules !== undefined) {
      args.push(`--host-resolver-rules=${options.hostRules}`);
    }
    const launch = (userAgent?: string) =>
      chromium.launchPersistentContext(
        fs.mkdtempSync(path.join(os.tmpdir(), "pwm-harness-")),
        {
          channel: "chromium",
          headless: true,
          ignoreHTTPSErrors: true,
          args,
          ...(userAgent === undefined ? {} : { userAgent }),
        },
      );
    let context = await launch();
    if (options.userAgentSuffix !== undefined) {
      const page = await context.newPage();
      const agent = await page.evaluate(() => navigator.userAgent);
      await context.close();
      context = await launch(`${agent} ${options.userAgentSuffix}`);
    }
    const worker =
      context.serviceWorkers()[0] ??
      (await context.waitForEvent("serviceworker"));
    const harness = new Harness(context, worker, "");
    await harness.inBackground(
      async ({ server, email, password }) => {
        const b = await globalThis.pwmTest.ready;
        await b.handle({
          kind: "setSettings",
          settings: { serverUrl: server, autoLockMinutes: 480 },
        });
        await b.handle({ kind: "signUp", email, password });
      },
      {
        server: SERVER,
        email: `harness-${String(Date.now())}@example.com`,
        password: "correct horse battery staple umbrella",
      },
    );
    return harness;
  }

  /** Runs `fn` in the extension's service worker, next to its background. */
  inBackground<T, A>(fn: (arg: A) => Promise<T>, arg: A): Promise<T> {
    return this.worker.evaluate(fn as (arg: unknown) => Promise<T>, arg);
  }

  /** Saves a login in the test vault; returns its item id. */
  async saveLogin(login: LoginSpec): Promise<string> {
    const saved = await this.inBackground(async (item) => {
      const b = await globalThis.pwmTest.ready;
      const now = Date.now();
      return b.saveItem(
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
    }, login);
    this.vaultId = saved.vaultId;
    return saved.itemId;
  }

  /** What the shortcut or the popup's Fill does, on the tab showing `page`. */
  fill(page: Page, item: string, confirmed = false): Promise<FillOutcome> {
    return this.inBackground(
      async ({ url, vault, item, confirmed }) => {
        const b = await globalThis.pwmTest.ready;
        const [tab] = await chrome.tabs.query({ url });
        if (tab?.id === undefined) return { code: "no tab", copiedCode: false };
        try {
          const result = await b.autofill.fillTab(
            tab.id,
            vault,
            item,
            confirmed,
          );
          return { code: "ok", copiedCode: result.copiedCode };
        } catch (error) {
          return {
            code: (error as { code?: string }).code ?? String(error),
            copiedCode: false,
          };
        }
      },
      { url: page.url(), vault: this.vaultId, item, confirmed },
    );
  }

  async close(): Promise<void> {
    await this.context.close();
  }
}

// The top-sites runner (D39 §2, §3): on each real login page, fill a
// throwaway login through the extension and see which inputs got it. It
// never submits, clicks, solves a challenge or types anything else, and it
// never uses a real account. Bot walls are recorded as blocked and left
// alone.
//
//   node runner/run.ts --sites sites/sites.json --out autofill-report.json
//                      [--timeout 20000] [--limit 50]
import fs from "node:fs";
import path from "node:path";
import type { Frame, Page } from "@playwright/test";
import { Harness } from "../lib/extension.ts";

export const CHECK_USER = "pwm-check-user@example.invalid";
export const CHECK_PASSWORD = "pwm-check-password";
export const USER_AGENT_SUFFIX = "PWM-AutofillCheck/1";

export interface SiteEntry {
  site: string;
  login_url: string;
  /** CSS selectors of the expected inputs, when someone checked the page. */
  username?: string;
  password?: string;
}

export type Status = "pass" | "fail" | "blocked" | "error";

export interface SiteResult {
  site: string;
  status: Status;
  ms: number;
  /** Why it failed, was blocked or errored (a code, never page text). */
  reason?: string;
  /** The inputs the engine would fill, as D38 report fields (no values). */
  targets?: unknown[];
}

export interface Report {
  v: 1;
  date: string;
  software: Record<string, unknown>;
  results: SiteResult[];
  totals: Record<Status, number> & { pass_rate: number };
}

const CHALLENGE_TITLE =
  /just a moment|attention required|access denied|verify (you are|you're) human|are you a robot|captcha|security check/i;
const CHALLENGE_SELECTORS = [
  ".cf-turnstile",
  ".g-recaptcha",
  ".h-captcha",
  "#px-captcha",
  "#challenge-form",
  'iframe[src*="recaptcha"]',
  'iframe[src*="hcaptcha"]',
  'iframe[src*="challenges.cloudflare.com"]',
  'iframe[title*="captcha" i]',
];

/** A bot wall or challenge page; never something to get past. */
async function looksBlocked(page: Page): Promise<boolean> {
  if (CHALLENGE_TITLE.test(await page.title())) return true;
  for (const selector of CHALLENGE_SELECTORS) {
    if ((await page.locator(selector).count()) > 0) return true;
  }
  return false;
}

/** Which inputs in a frame hold the check values, and whether it has a visible password input. */
function inspect(frame: Frame) {
  return frame
    .evaluate(
      ({ user, password }) => {
        const inputs = [...document.querySelectorAll("input")];
        const visible = (el: HTMLInputElement) => {
          const r = el.getBoundingClientRect();
          return (
            r.width > 1 &&
            r.height > 1 &&
            getComputedStyle(el).visibility === "visible"
          );
        };
        return {
          user: inputs.some(
            (el) => el.type !== "password" && el.value === user,
          ),
          password: inputs.some(
            (el) => el.type === "password" && el.value === password,
          ),
          passwordField: inputs.some(
            (el) => el.type === "password" && visible(el),
          ),
        };
      },
      { user: CHECK_USER, password: CHECK_PASSWORD },
    )
    .catch(() => ({ user: false, password: false, passwordField: false }));
}

/** Runs one site; never throws. */
export async function runSite(
  harness: Harness,
  entry: SiteEntry,
  timeoutMs = 20_000,
): Promise<SiteResult> {
  const started = Date.now();
  const done = (
    status: Status,
    extra: Partial<SiteResult> = {},
  ): SiteResult => ({
    site: entry.site,
    status,
    ms: Date.now() - started,
    ...extra,
  });
  let page: Page | undefined;
  try {
    const origin = new URL(entry.login_url).origin;
    const item = await harness.saveLogin({
      title: entry.site,
      username: CHECK_USER,
      password: CHECK_PASSWORD,
      url: origin,
    });
    page = await harness.context.newPage();
    let response;
    try {
      response = await page.goto(entry.login_url, {
        timeout: timeoutMs,
        waitUntil: "domcontentloaded",
      });
    } catch {
      return done("error", { reason: "timeout" });
    }
    if (response && response.status() >= 400) {
      return done("blocked", { reason: `http_${String(response.status())}` });
    }
    // Pages that draw their form with script need a moment.
    await page
      .waitForLoadState("load", { timeout: 5_000 })
      .catch(() => undefined);
    await page.waitForTimeout(1_500);
    if (await looksBlocked(page))
      return done("blocked", { reason: "challenge" });

    await page.bringToFront();
    const outcome = await harness.fill(page, item);
    const targets = await harness
      .inBackground(async () => {
        const b = await globalThis.pwmTest.ready;
        const preview = (await b.handle({
          kind: "reportPreview",
          problem: "other",
        })) as { json: string };
        const report = JSON.parse(preview.json) as {
          frames: { fields: { fill: boolean }[] }[];
        };
        return report.frames.flatMap((f) => f.fields.filter((x) => x.fill));
      }, null)
      .catch(() => undefined);

    let ok: boolean;
    if (entry.username !== undefined) {
      const user = await page
        .locator(entry.username)
        .first()
        .inputValue()
        .catch(() => "");
      const password =
        entry.password === undefined
          ? CHECK_PASSWORD
          : await page
              .locator(entry.password)
              .first()
              .inputValue()
              .catch(() => "");
      ok = user === CHECK_USER && password === CHECK_PASSWORD;
    } else {
      const frames = await Promise.all(page.frames().map(inspect));
      const user = frames.some((f) => f.user);
      const password = frames.some((f) => f.password);
      const passwordField = frames.some((f) => f.passwordField);
      // Multi-step sites ask for the username first; that page is the pass.
      ok = user && (password || !passwordField);
    }
    const extra: Partial<SiteResult> = {};
    if (targets !== undefined) extra.targets = targets;
    if (!ok)
      extra.reason = outcome.code === "ok" ? "wrong_inputs" : outcome.code;
    return done(ok ? "pass" : "fail", extra);
  } catch {
    return done("error", { reason: "runner" });
  } finally {
    await page?.close().catch(() => undefined);
  }
}

export function summarize(
  results: SiteResult[],
  software: Record<string, unknown>,
): Report {
  const count = (s: Status) => results.filter((r) => r.status === s).length;
  const pass = count("pass");
  const fail = count("fail");
  return {
    v: 1,
    date: new Date().toISOString(),
    software,
    results,
    totals: {
      pass,
      fail,
      blocked: count("blocked"),
      error: count("error"),
      // Blocked and errored sites say nothing about autofill.
      pass_rate:
        pass + fail === 0 ? 0 : Math.round((1000 * pass) / (pass + fail)) / 10,
    },
  };
}

async function main(): Promise<void> {
  const arg = (name: string): string | undefined => {
    const i = process.argv.indexOf(`--${name}`);
    return i > 0 ? process.argv[i + 1] : undefined;
  };
  const sitesFile = arg("sites") ?? "sites/sites.json";
  const out = arg("out") ?? "autofill-report.json";
  const timeout = Number(arg("timeout") ?? "20000");
  const limit = Number(arg("limit") ?? "100000");
  const list = (
    JSON.parse(fs.readFileSync(path.resolve(sitesFile), "utf8")) as {
      sites: SiteEntry[];
    }
  ).sites.slice(0, limit);
  const harness = await Harness.start({ userAgentSuffix: USER_AGENT_SUFFIX });
  const results: SiteResult[] = [];
  for (const entry of list) {
    const result = await runSite(harness, entry, timeout);
    results.push(result);
    console.log(
      `${result.status.padEnd(7)} ${entry.site} (${String(result.ms)} ms)${result.reason ? ` ${result.reason}` : ""}`,
    );
  }
  const agent = await harness.inBackground(
    () => Promise.resolve(navigator.userAgent),
    null,
  );
  const pkg = JSON.parse(
    fs.readFileSync(
      path.resolve(import.meta.dirname, "../../../extension/package.json"),
      "utf8",
    ),
  ) as { version: string };
  const report = summarize(results, {
    extension: pkg.version,
    browser: /Chrome\/(\d+)/.exec(agent)?.[1] ?? "unknown",
  });
  await harness.close();
  fs.writeFileSync(out, JSON.stringify(report, null, 2) + "\n");
  console.log(
    `pass rate ${String(report.totals.pass_rate)}% (${String(report.totals.pass)} pass, ${String(report.totals.fail)} fail, ${String(report.totals.blocked)} blocked, ${String(report.totals.error)} error)`,
  );
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === path.resolve(import.meta.filename)
) {
  await main();
}

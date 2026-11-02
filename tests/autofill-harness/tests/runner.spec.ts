// The top-sites runner against local pages (D39 §5): each status, that it
// never submits, the report's totals, and the release gate.
import { expect, test } from "@playwright/test";
import { Harness } from "../lib/extension.ts";
import { compare } from "../runner/compare.ts";
import type { Report } from "../runner/run.ts";
import { CHECK_USER, runSite, summarize } from "../runner/run.ts";

const R = "https://runner.test:8443";

let harness: Harness;
const requested: string[] = [];

test.beforeAll(async () => {
  harness = await Harness.start({ hostRules: "MAP *.test 127.0.0.1" });
  harness.context.on("request", (r) => requested.push(r.url()));
});

test.afterAll(async () => {
  await harness.close();
});

const site = (page: string, extra: object = {}) => ({
  site: `runner.test/${page}`,
  login_url: `${R}/${page}`,
  ...extra,
});

test("runner: pass with the expected inputs", async () => {
  const result = await runSite(
    harness,
    site("plain.html", { username: "#email", password: "#password" }),
  );
  expect(result).toMatchObject({ status: "pass" });
  expect(result.targets).toHaveLength(2);
  expect(JSON.stringify(result)).not.toContain(CHECK_USER);
});

test("runner: pass without selectors, and on a username-first page", async () => {
  expect((await runSite(harness, site("honeypot.html"))).status).toBe("pass");
  expect((await runSite(harness, site("multistep1.html"))).status).toBe("pass");
});

test("runner: fail when nothing or the wrong inputs fill", async () => {
  expect(await runSite(harness, site("no-form.html"))).toMatchObject({
    status: "fail",
    reason: "no_form",
  });
  expect(
    await runSite(harness, site("plain.html", { username: "#nope" })),
  ).toMatchObject({ status: "fail", reason: "wrong_inputs" });
  expect(await runSite(harness, site("overlay.html"))).toMatchObject({
    status: "fail",
    reason: "field_hidden",
  });
});

test("runner: blocked by a challenge page or an error status, left alone", async () => {
  expect(await runSite(harness, site("captcha.html"))).toMatchObject({
    status: "blocked",
    reason: "challenge",
  });
  expect(await runSite(harness, site("status-503"))).toMatchObject({
    status: "blocked",
    reason: "http_503",
  });
});

test("runner: error on a page that does not load in time", async () => {
  expect(await runSite(harness, site("slow"), 2_000)).toMatchObject({
    status: "error",
    reason: "timeout",
  });
});

test("runner: never submits", async () => {
  expect((await runSite(harness, site("submit-trap.html"))).status).toBe(
    "pass",
  );
  expect(
    requested.some(
      (u) => u.includes("trap.html") && !u.includes("submit-trap"),
    ),
  ).toBe(false);
});

function report(rate: number, statuses: Record<string, string>): Report {
  return {
    v: 1,
    date: "2026-11-02T00:00:00Z",
    software: {},
    results: Object.entries(statuses).map(([s, status]) => ({
      site: s,
      status: status as "pass",
      ms: 1,
    })),
    totals: { pass: 0, fail: 0, blocked: 0, error: 0, pass_rate: rate },
  };
}

test("runner: totals leave blocked and errored sites out of the pass rate", () => {
  const r = summarize(
    [
      { site: "a", status: "pass", ms: 1 },
      { site: "b", status: "pass", ms: 1 },
      { site: "c", status: "fail", ms: 1 },
      { site: "d", status: "blocked", ms: 1 },
      { site: "e", status: "error", ms: 1 },
    ],
    {},
  );
  expect(r.totals).toEqual({
    pass: 2,
    fail: 1,
    blocked: 1,
    error: 1,
    pass_rate: 66.7,
  });
  expect(r.v).toBe(1);
});

test("runner: the release gate allows a 0.4 point drop and stops a 0.6 point one", () => {
  const before = report(95.0, { a: "pass", b: "pass" });
  expect(compare(before, report(94.6, { a: "pass", b: "fail" }))).toMatchObject(
    {
      drop: 0.4,
      ok: true,
      changed: [{ site: "b", from: "pass", to: "fail" }],
    },
  );
  expect(compare(before, report(94.4, { a: "pass", b: "pass" }))).toMatchObject(
    {
      drop: 0.6,
      ok: false,
    },
  );
  expect(compare(before, report(96, {})).ok).toBe(true);
});

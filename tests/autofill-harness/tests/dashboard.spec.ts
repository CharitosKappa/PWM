// The public autofill dashboard (D77): the page built from a report and the
// published history. No browser: these run anywhere.
import fs from "node:fs";
import path from "node:path";
import { expect, test } from "@playwright/test";
import {
  addNight,
  badNight,
  build,
  checkHistory,
  checkReport,
  type History,
  NIGHTS_KEPT,
  reasonText,
  rows,
  showDay,
  since,
  STATUS_TEXT,
  totals,
} from "../dashboard/build.ts";
import type { Status } from "../runner/run.ts";

const tokens = JSON.parse(
  fs.readFileSync(
    path.resolve(import.meta.dirname, "../../../design/tokens.json"),
    "utf8",
  ),
) as Parameters<typeof build>[2];

/** A runner report (v1) for one night, its totals worked out as the runner's. */
function report(
  day: string,
  statuses: Record<string, Status>,
  reasons: Record<string, string> = {},
) {
  const results = Object.entries(statuses).map(([site, status]) => ({
    site,
    status,
    ms: 1,
    ...(reasons[site] === undefined ? {} : { reason: reasons[site] }),
  }));
  return {
    v: 1,
    date: `${day}T03:20:00.000Z`,
    software: { extension: "0.1.0", browser: "141" },
    results,
    totals: totals(results.map((r) => r.status)),
  };
}

const empty: History = { v: 1, nights: [] };

/** History after the given nights, each a day's statuses. */
function nights(...days: [string, Record<string, Status>][]): History {
  return days.reduce(
    (h, [day, statuses]) => addNight(h, checkReport(report(day, statuses))),
    empty,
  );
}

test("dashboard: the pass rate and counts are the report's", () => {
  const night = report("2026-11-08", {
    "a.com": "pass",
    "b.com": "pass",
    "c.com": "pass",
    "d.com": "fail",
    "e.com": "blocked",
    "f.com": "error",
  });
  const files = build(checkReport(night), empty, tokens);
  const html = files["index.html"] ?? "";
  expect(night.totals.pass_rate).toBe(75);
  expect(html).toContain('<p class="rate-number">75%</p>');
  expect(html).toContain(
    '<div class="count count-pass"><dt>Works</dt><dd>3</dd></div>',
  );
  expect(html).toContain("<dt>Doesn&#39;t work</dt><dd>1</dd>");
  expect(html).toContain("<dt>Not tested</dt><dd>1</dd>");
  expect(html).toContain("<dt>Couldn&#39;t load</dt><dd>1</dd>");
  const published = JSON.parse(files["report.json"] ?? "") as {
    v: number;
    totals: unknown;
    sites: unknown[];
  };
  expect(published.v).toBe(1);
  expect(published.totals).toEqual(night.totals);
  expect(published.sites).toHaveLength(6);
});

test("dashboard: totals that don't match the sites stop the build", () => {
  const night = report("2026-11-08", { "a.com": "pass", "b.com": "fail" });
  night.totals.pass_rate = 100;
  expect(() => checkReport(night)).toThrow(/totals/);
});

test("dashboard: a report that fails its schema stops the build", () => {
  const good = report("2026-11-08", { "a.com": "pass" });
  for (const broken of [
    null,
    { ...good, v: 2 },
    { ...good, date: "yesterday" },
    { ...good, results: [{ site: "a.com", status: "maybe", ms: 1 }] },
    { ...good, results: [{ site: "", status: "pass", ms: 1 }] },
    { ...good, results: [{ site: 5, status: "pass", ms: 1 }] },
    { ...good, software: { extension: { nested: true } } },
  ])
    expect(() => checkReport(broken)).toThrow();
  const twice = report("2026-11-08", { "a.com": "pass" });
  twice.results.push({ site: "a.com", status: "pass", ms: 1 });
  twice.totals = totals(["pass", "pass"]);
  expect(() => checkReport(twice)).toThrow(/twice/);
});

test("dashboard: since is the first night of the current status", () => {
  const history = nights(
    ["2026-11-01", { "same.com": "pass", "changed.com": "pass" }],
    ["2026-11-02", { "same.com": "pass", "changed.com": "fail" }],
    [
      "2026-11-03",
      { "same.com": "pass", "changed.com": "fail", "new.com": "fail" },
    ],
  );
  expect(since(history.nights, "same.com")).toBe("2026-11-01");
  expect(since(history.nights, "changed.com")).toBe("2026-11-02");
  expect(since(history.nights, "new.com")).toBe("2026-11-03");
  // A night the site was missing ends the run of nights.
  const gap = nights(
    ["2026-11-01", { "gap.com": "fail" }],
    ["2026-11-02", {}],
    ["2026-11-03", { "gap.com": "fail" }],
  );
  expect(since(gap.nights, "gap.com")).toBe("2026-11-03");
});

test("dashboard: the fix mark comes after more than 7 days", () => {
  const days = (n: number) =>
    Array.from({ length: n }, (_, i): [string, Record<string, Status>] => [
      `2026-11-${String(i + 1).padStart(2, "0")}`,
      { "broken.com": "fail", "fine.com": "pass" },
    ]);
  // Failing since 1 Nov: on 8 Nov that is 7 days, on 9 Nov more.
  const seven = nights(...days(8));
  const tonight7 = checkReport(
    report("2026-11-08", { "broken.com": "fail", "fine.com": "pass" }),
  );
  expect(
    rows(tonight7, seven).find((r) => r.site === "broken.com"),
  ).toMatchObject({
    since: "2026-11-01",
    over_sla: false,
  });
  const eight = nights(...days(9));
  const tonight8 = checkReport(
    report("2026-11-09", { "broken.com": "fail", "fine.com": "pass" }),
  );
  const marked = rows(tonight8, eight);
  expect(marked.find((r) => r.site === "broken.com")?.over_sla).toBe(true);
  expect(marked.find((r) => r.site === "fine.com")?.over_sla).toBe(false);
  const html = build(tonight8, nights(...days(8)), tokens)["index.html"] ?? "";
  expect(html).toContain('<span class="sla">Over 7 days</span>');
});

test("dashboard: sites that don't work come first, then by name", () => {
  const night = checkReport(
    report("2026-11-08", {
      "zeta.com": "fail",
      "beta.com": "pass",
      "alpha.com": "blocked",
      "gamma.com": "fail",
    }),
  );
  expect(rows(night, addNight(empty, night)).map((r) => r.site)).toEqual([
    "gamma.com",
    "zeta.com",
    "alpha.com",
    "beta.com",
  ]);
});

test("dashboard: every reason code has words, and an unknown one is Other", () => {
  for (const code of [
    "wrong_inputs",
    "no_form",
    "field_hidden",
    "field_hidden:covered",
    "refused",
    "page_changed",
    "needs_confirm",
    "no_page",
    "no tab",
    "challenge",
    "bot_wall",
    "timeout",
    "navigation",
    "runner",
    "locked",
    "unknown_item",
    "http_403",
    "net::ERR_HTTP2_PROTOCOL_ERROR",
  ]) {
    const text = reasonText(code);
    expect(text, code).not.toBe("Other");
    expect(text, code).not.toBe("");
  }
  expect(reasonText("http_403")).toContain("403");
  expect(reasonText("something_new")).toBe("Other");
  expect(reasonText(undefined)).toBe("");
  expect(STATUS_TEXT).toEqual({
    pass: "Works",
    fail: "Doesn't work",
    blocked: "Not tested",
    error: "Couldn't load",
  });
});

test("dashboard: text from the report is escaped", () => {
  const night = checkReport(
    report(
      "2026-11-08",
      { '<script>alert("x")</script>.com': "fail" },
      { '<script>alert("x")</script>.com': "<b>new</b>" },
    ),
  );
  night.software = { extension: "<img src=x>" };
  const html = build(night, empty, tokens)["index.html"] ?? "";
  expect(html).toContain(
    "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;.com",
  );
  expect(html).toContain("extension &lt;img src=x&gt;");
  expect(html).not.toContain("<script");
  expect(html).not.toContain("<img");
  expect(html).not.toContain("<b>");
});

test("dashboard: the page has no scripts and asks no other host", () => {
  const files = build(
    checkReport(report("2026-11-08", { "a.com": "pass", "b.com": "fail" })),
    empty,
    tokens,
  );
  const html = files["index.html"] ?? "";
  const style = files["style.css"] ?? "";
  expect(html).not.toMatch(/<script|javascript:|\son[a-z]+=/i);
  expect(html).not.toMatch(/https?:\/\/|\/\/[a-z]/i);
  expect(style).not.toMatch(/https?:|@import|url\(/i);
  expect(html).toContain(
    `content="default-src 'none'; style-src 'self'; img-src 'self'"`,
  );
  expect(html).not.toContain(" style=");
  // The colors are the design's, in both themes.
  expect(style).toContain("--accent: #4F3FD6;");
  expect(style).toContain("--accent: #A79BFF;");
});

test("dashboard: history keeps 90 nights and drops the oldest", () => {
  let history = empty;
  const start = Date.parse("2026-01-01T00:00:00Z");
  for (let i = 0; i < NIGHTS_KEPT + 5; i++) {
    const day = new Date(start + i * 86_400_000).toISOString().slice(0, 10);
    history = addNight(history, checkReport(report(day, { "a.com": "pass" })));
  }
  expect(NIGHTS_KEPT).toBe(90);
  expect(history.nights).toHaveLength(90);
  expect(history.nights[0]?.date).toBe("2026-01-06");
  // A second run the same day replaces that night.
  const again = addNight(
    history,
    checkReport(report("2026-04-05", { "a.com": "fail" })),
  );
  expect(again.nights).toHaveLength(90);
  expect(again.nights.at(-1)).toMatchObject({
    date: "2026-04-05",
    pass_rate: 0,
  });
  // An older night than the history's last is refused.
  expect(() =>
    addNight(history, checkReport(report("2026-02-01", { "a.com": "pass" }))),
  ).toThrow(/after/);
  // What the page publishes reads back as the same history.
  const files = build(
    checkReport(report("2026-04-06", { "a.com": "pass" })),
    history,
    tokens,
  );
  const read = checkHistory(JSON.parse(files["history.json"] ?? ""));
  expect(read.nights).toHaveLength(90);
  expect(read.nights.at(-1)?.date).toBe("2026-04-06");
});

test("dashboard: a broken history stops the build", () => {
  for (const broken of [
    null,
    { v: 2, nights: [] },
    { v: 1 },
    { v: 1, nights: [{ date: "2026-11-01", pass_rate: 101, sites: {} }] },
    {
      v: 1,
      nights: [{ date: "2026-11-01", pass_rate: 50, sites: { "a.com": "ok" } }],
    },
    {
      v: 1,
      nights: [
        { date: "2026-11-02", pass_rate: 50, sites: {} },
        { date: "2026-11-01", pass_rate: 50, sites: {} },
      ],
    },
  ])
    expect(() => checkHistory(broken)).toThrow();
});

test("dashboard: a night with more than 20% errors isn't published", () => {
  const night = (errors: number) => {
    const statuses: Record<string, Status> = {};
    for (let i = 0; i < 100; i++)
      statuses[`s${String(i)}.com`] = i < errors ? "error" : "pass";
    return checkReport(report("2026-11-08", statuses));
  };
  expect(badNight(night(20))).toBe(false);
  expect(badNight(night(21))).toBe(true);
  // Bot checks are the sites', not ours.
  const blocked: Record<string, Status> = {};
  for (let i = 0; i < 10; i++)
    blocked[`s${String(i)}.com`] = i < 6 ? "blocked" : "pass";
  expect(badNight(checkReport(report("2026-11-08", blocked)))).toBe(false);
  expect(badNight(checkReport(report("2026-11-08", {})))).toBe(true);
});

test("dashboard: dates read the same on every machine", () => {
  expect(showDay("2026-11-08")).toBe("8 Nov 2026");
  expect(showDay("2027-01-31")).toBe("31 Jan 2027");
});

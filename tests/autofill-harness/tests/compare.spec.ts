// The release gate (D64): it compares the sites both reports counted, keeps
// the 0.5 point limit on them, and fails when too few sites are in common.
// No browser: these run anywhere.
import { expect, test } from "@playwright/test";
import { compare, MAX_DROP, MIN_OVERLAP } from "../runner/compare.ts";
import type { Report, Status } from "../runner/run.ts";
import { summarize } from "../runner/run.ts";

/** A report whose totals are worked out as the runner's are. */
function report(statuses: Record<string, Status>): Report {
  return summarize(
    Object.entries(statuses).map(([site, status]) => ({ site, status, ms: 1 })),
    {},
  );
}

/** `n` sites named `prefix1`…, the first `passing` of them passing. */
function sites(
  n: number,
  passing: number,
  prefix = "s",
): Record<string, Status> {
  const out: Record<string, Status> = {};
  for (let i = 1; i <= n; i++)
    out[`${prefix}${String(i)}`] = i <= passing ? "pass" : "fail";
  return out;
}

test("gate: the limit is 0.5 points and 80% of the sites", () => {
  expect(MAX_DROP).toBe(0.5);
  expect(MIN_OVERLAP).toBe(0.8);
});

test("gate: a site moving to blocked or error doesn't move it", () => {
  // As on 2026-11-06: the same code, three sites moving in or out of the count.
  const before = report({
    ...sites(48, 28),
    asana: "pass",
    trello: "blocked",
    instagram: "error",
    zalando: "blocked",
  });
  const after = report({
    ...sites(48, 28),
    asana: "blocked",
    trello: "fail",
    instagram: "pass",
    zalando: "error",
  });
  expect(before.totals.pass_rate).not.toBe(after.totals.pass_rate);
  const result = compare(before, after);
  expect(result).toMatchObject({
    before: before.totals.pass_rate,
    after: after.totals.pass_rate,
    common: { sites: 48, before: 58.3, after: 58.3, drop: 0 },
    overlap: { counted: 48, of: 49 },
    ok: true,
    lost: [],
    gained: [],
  });
  expect(result.problem).toBeUndefined();
  expect(result.left_out).toEqual([
    { site: "asana", from: "pass", to: "blocked" },
    { site: "trello", from: "blocked", to: "fail" },
    { site: "instagram", from: "error", to: "pass" },
    { site: "zalando", from: "blocked", to: "error" },
  ]);
});

test("gate: one site lost among 48 stops it; one gained as well lets it through", () => {
  const before = report(sites(48, 28));
  const lost = report({ ...sites(48, 28), s1: "fail" });
  expect(compare(before, lost)).toMatchObject({
    common: { sites: 48, drop: 2.1 },
    ok: false,
    problem: "drop",
    lost: ["s1"],
    gained: [],
  });
  const traded = report({ ...sites(48, 28), s1: "fail", s48: "pass" });
  expect(compare(before, traded)).toMatchObject({
    common: { drop: 0 },
    ok: true,
    lost: ["s1"],
    gained: ["s48"],
  });
});

test("gate: sites added to or removed from the list are left out", () => {
  const before = report({ ...sites(10, 8), removed: "fail", gone: "pass" });
  const after = report({ ...sites(10, 8), added: "fail" });
  const result = compare(before, after);
  expect(result).toMatchObject({
    common: { sites: 10, before: 80, after: 80, drop: 0 },
    overlap: { counted: 10, of: 10 },
    ok: true,
    lost: [],
  });
  expect(result.left_out).toEqual([
    { site: "removed", from: "fail", to: "" },
    { site: "gone", from: "pass", to: "" },
    { site: "added", from: "", to: "fail" },
  ]);
});

test("gate: fewer than 80% of the sites in common stops it", () => {
  const before = report(sites(10, 5));
  const blocked: Record<string, Status> = {
    s1: "blocked",
    s2: "blocked",
    s3: "error",
  };
  // 7 of 10 counted again: the rate is unchanged, but there is too little to compare.
  expect(
    compare(before, report({ ...sites(10, 5), ...blocked })),
  ).toMatchObject({
    overlap: { counted: 7, of: 10 },
    ok: false,
    problem: "overlap",
  });
  // 8 of 10 is enough.
  expect(
    compare(before, report({ ...sites(10, 5), s1: "blocked", s2: "error" })),
  ).toMatchObject({ overlap: { counted: 8, of: 10 }, ok: true });
  // A previous report that counted nothing can't be compared with.
  const nothing = report({ a: "blocked" });
  expect(compare(nothing, report(sites(10, 5)))).toMatchObject({
    overlap: { counted: 0, of: 0 },
    ok: false,
    problem: "overlap",
  });
});

test("gate: a 0.4 point drop on the common sites passes, a 0.6 point one stops it", () => {
  // One site of 250 is 0.4 points; one of 166 is 0.6.
  expect(
    compare(
      report(sites(250, 200)),
      report({ ...sites(250, 200), s1: "fail" }),
    ),
  ).toMatchObject({ common: { drop: 0.4 }, ok: true, lost: ["s1"] });
  expect(
    compare(
      report(sites(166, 100)),
      report({ ...sites(166, 100), s1: "fail" }),
    ),
  ).toMatchObject({ common: { drop: 0.6 }, ok: false, problem: "drop" });
  // Better is always fine.
  expect(
    compare(report(sites(48, 28)), report({ ...sites(48, 28), s48: "pass" })),
  ).toMatchObject({ ok: true, gained: ["s48"] });
});

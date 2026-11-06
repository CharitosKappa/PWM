// The release gate (D64): compares two runner reports on the sites both
// counted (pass or fail in each), and fails when the pass rate on those
// dropped more than 0.5 points, or when too few sites are in common.
//
//   node runner/compare.ts previous.json candidate.json
import fs from "node:fs";
import path from "node:path";
import type { Report, Status } from "./run.ts";

/** Percentage points the pass rate on the common sites may drop. */
export const MAX_DROP = 0.5;

/**
 * The share of the sites the previous report counted (and that are still
 * in the candidate's) that the candidate must count too.
 */
export const MIN_OVERLAP = 0.8;

export interface Comparison {
  /** The whole-list pass rates, as the reports give them. */
  before: number;
  after: number;
  /** The gate's rate: the sites both reports counted. */
  common: { sites: number; before: number; after: number; drop: number };
  /** Of the sites the previous report counted and the candidate still has, how many it counted. */
  overlap: { counted: number; of: number };
  ok: boolean;
  /** Why the gate stopped: too few sites in common, or the drop. */
  problem?: "overlap" | "drop";
  /** Common sites that went from pass to fail, and from fail to pass. */
  lost: string[];
  gained: string[];
  /**
   * Sites left out of the count whose status changed or that are in one
   * report only ("" for the missing side). Sites blocked or errored the
   * same way in both aren't listed.
   */
  left_out: { site: string; from: Status | ""; to: Status | "" }[];
}

const counted = (s: Status | undefined): boolean =>
  s === "pass" || s === "fail";

const rate = (pass: number, of: number): number =>
  of === 0 ? 0 : (100 * pass) / of;

const tenth = (n: number): number => Math.round(n * 10) / 10;

export function compare(before: Report, after: Report): Comparison {
  const old = new Map(before.results.map((r) => [r.site, r.status]));
  const now = new Map(after.results.map((r) => [r.site, r.status]));

  const common: string[] = [];
  const left_out: Comparison["left_out"] = [];
  let of = 0;
  for (const r of before.results) {
    const to = now.get(r.site);
    if (counted(r.status) && to !== undefined) of++;
    if (counted(r.status) && counted(to)) common.push(r.site);
    else if (to !== r.status)
      left_out.push({ site: r.site, from: r.status, to: to ?? "" });
  }
  for (const r of after.results)
    if (!old.has(r.site))
      left_out.push({ site: r.site, from: "", to: r.status });

  const passing = (statuses: Map<string, Status>) =>
    common.filter((s) => statuses.get(s) === "pass").length;
  const was = rate(passing(old), common.length);
  const is = rate(passing(now), common.length);
  const drop = tenth(was - is);

  const problem =
    of === 0 || common.length < MIN_OVERLAP * of
      ? ("overlap" as const)
      : drop > MAX_DROP
        ? ("drop" as const)
        : undefined;
  return {
    before: before.totals.pass_rate,
    after: after.totals.pass_rate,
    common: {
      sites: common.length,
      before: tenth(was),
      after: tenth(is),
      drop,
    },
    overlap: { counted: common.length, of },
    ok: problem === undefined,
    ...(problem === undefined ? {} : { problem }),
    lost: common.filter((s) => old.get(s) === "pass" && now.get(s) === "fail"),
    gained: common.filter(
      (s) => old.get(s) === "fail" && now.get(s) === "pass",
    ),
    left_out,
  };
}

const points = (drop: number): string =>
  drop === 0
    ? "no change"
    : `${drop > 0 ? "down" : "up"} ${String(Math.abs(drop))} points`;

function main(): void {
  const [beforeFile, afterFile] = process.argv.slice(2);
  if (beforeFile === undefined || afterFile === undefined) {
    console.error("usage: node runner/compare.ts previous.json candidate.json");
    process.exit(2);
  }
  const read = (f: string) => JSON.parse(fs.readFileSync(f, "utf8")) as Report;
  const r = compare(read(beforeFile), read(afterFile));
  for (const s of r.lost) console.log(`lost: ${s} (pass -> fail)`);
  for (const s of r.gained) console.log(`gained: ${s} (fail -> pass)`);
  for (const s of r.left_out)
    console.log(
      `left out: ${s.site} (${s.from || "not listed"} -> ${s.to || "not listed"})`,
    );
  console.log(
    `whole list: ${String(r.before)}% -> ${String(r.after)}% (${points(tenth(r.before - r.after))})`,
  );
  console.log(
    `gate, on the ${String(r.common.sites)} sites both runs counted: ${String(r.common.before)}% -> ${String(r.common.after)}% (${points(r.common.drop)}; it allows a drop of at most ${String(MAX_DROP)})`,
  );
  console.log(
    `overlap: ${String(r.overlap.counted)} of ${String(r.overlap.of)} sites (at least ${String(MIN_OVERLAP * 100)}% needed)`,
  );
  if (r.problem === "overlap") console.log("stopped: too few sites to compare");
  else if (r.problem === "drop") console.log("stopped: the pass rate dropped");
  if (!r.ok) process.exit(1);
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === path.resolve(import.meta.filename)
) {
  main();
}

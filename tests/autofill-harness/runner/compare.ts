// The release gate (D39 §3, QUALITY.md): compares two runner reports and
// fails when the pass rate dropped more than 0.5 points.
//
//   node runner/compare.ts previous.json candidate.json
import fs from "node:fs";
import path from "node:path";
import type { Report } from "./run.ts";

/** Percentage points the pass rate may drop between releases. */
export const MAX_DROP = 0.5;

export interface Comparison {
  before: number;
  after: number;
  drop: number;
  ok: boolean;
  /** Sites whose status changed. */
  changed: { site: string; from: string; to: string }[];
}

export function compare(before: Report, after: Report): Comparison {
  const old = new Map(before.results.map((r) => [r.site, r.status]));
  const changed = after.results
    .filter((r) => old.has(r.site) && old.get(r.site) !== r.status)
    .map((r) => ({ site: r.site, from: old.get(r.site) ?? "", to: r.status }));
  const drop =
    Math.round((before.totals.pass_rate - after.totals.pass_rate) * 10) / 10;
  return {
    before: before.totals.pass_rate,
    after: after.totals.pass_rate,
    drop,
    ok: drop <= MAX_DROP,
    changed,
  };
}

function main(): void {
  const [beforeFile, afterFile] = process.argv.slice(2);
  if (beforeFile === undefined || afterFile === undefined) {
    console.error("usage: node runner/compare.ts previous.json candidate.json");
    process.exit(2);
  }
  const read = (f: string) => JSON.parse(fs.readFileSync(f, "utf8")) as Report;
  const result = compare(read(beforeFile), read(afterFile));
  for (const c of result.changed)
    console.log(`${c.site}: ${c.from} -> ${c.to}`);
  console.log(
    `pass rate ${String(result.before)}% -> ${String(result.after)}% (drop ${String(result.drop)} points, at most ${String(MAX_DROP)})`,
  );
  if (!result.ok) process.exit(1);
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === path.resolve(import.meta.filename)
) {
  main();
}

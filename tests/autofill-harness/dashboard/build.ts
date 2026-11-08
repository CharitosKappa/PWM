// The public autofill dashboard (D77): a night's runner report and the
// published history become a static page: plain HTML and CSS, prerendered,
// with no scripts, cookies, fonts or requests to other hosts.
//
//   node dashboard/build.ts --report autofill-report.json
//                           (--history history.json | --first) --out site
//                           [--tranco]
//
// `--history` is the history.json downloaded from the live page; `--first`
// starts an empty one, for the very first deploy only. `--tranco` adds the
// Tranco attribution, once the site list is ordered by it (D39 §2).
import fs from "node:fs";
import path from "node:path";
import type { Report, Status } from "../runner/run.ts";

/** The product's name on the page: "PWM" until D1 picks one. */
export const PRODUCT = "PWM";
/** Nights kept in history.json, and drawn in the trend. */
export const NIGHTS_KEPT = 90;
/** A site that hasn't worked for more than this is marked (AUTOFILL.md). */
export const SLA_DAYS = 7;
/** More of a night's sites than this ending as `error` means the runner broke. */
export const MAX_ERROR_SHARE = 0.2;
/** The pass rate targets (D39): now, and by launch. */
export const TARGET = 90;
export const LAUNCH_TARGET = 95;

const STATUSES: readonly Status[] = ["pass", "fail", "blocked", "error"];

export const STATUS_TEXT: Record<Status, string> = {
  pass: "Works",
  fail: "Doesn't work",
  blocked: "Not tested",
  error: "Couldn't load",
};

/** One night in history.json. */
export interface Night {
  /** The run's day, `YYYY-MM-DD` (UTC). */
  date: string;
  pass_rate: number;
  sites: Record<string, Status>;
}

export interface History {
  v: 1;
  nights: Night[];
}

/** One site on the page and in the published report.json. */
export interface Row {
  site: string;
  status: Status;
  /** The runner's code, when it gave one. */
  reason?: string;
  /** The first of the consecutive nights with this status. */
  since: string;
  /** Not working for more than SLA_DAYS. */
  over_sla: boolean;
}

/** The checked report: only the fields the page uses. */
export interface CheckedReport {
  date: string;
  software: Record<string, string>;
  results: { site: string; status: Status; reason?: string }[];
  totals: Report["totals"];
}

export interface BuildOptions {
  /** Show the Tranco attribution (D39 §2). */
  tranco?: boolean;
}

class BadInput extends Error {}

function bad(what: string): never {
  throw new BadInput(what);
}

function isRecord(x: unknown): x is Record<string, unknown> {
  return typeof x === "object" && x !== null && !Array.isArray(x);
}

const isStatus = (x: unknown): x is Status =>
  typeof x === "string" && (STATUSES as readonly string[]).includes(x);

const DAY = /^\d{4}-\d{2}-\d{2}$/;

/** Counts and the pass rate, worked out as the runner does (D39 §3). */
export function totals(statuses: Status[]): Report["totals"] {
  const count = (s: Status) => statuses.filter((x) => x === s).length;
  const pass = count("pass");
  const fail = count("fail");
  return {
    pass,
    fail,
    blocked: count("blocked"),
    error: count("error"),
    pass_rate:
      pass + fail === 0 ? 0 : Math.round((1000 * pass) / (pass + fail)) / 10,
  };
}

/**
 * Checks a runner report (v1) and keeps what the page needs. Anything
 * unexpected stops the build, including totals that don't match the sites.
 */
export function checkReport(input: unknown): CheckedReport {
  if (!isRecord(input)) bad("report: not an object");
  if (input.v !== 1) bad("report: not version 1");
  const date = input.date;
  if (
    typeof date !== "string" ||
    !DAY.test(date.slice(0, 10)) ||
    Number.isNaN(Date.parse(date))
  )
    bad("report: bad date");
  const software: Record<string, string> = {};
  if (!isRecord(input.software)) bad("report: bad software");
  for (const [key, value] of Object.entries(input.software)) {
    if (typeof value !== "string" && typeof value !== "number")
      bad(`report: bad software.${key}`);
    software[key.slice(0, 40)] = String(value).slice(0, 40);
  }
  if (!Array.isArray(input.results) || input.results.length > 5000)
    bad("report: bad results");
  const list: unknown[] = input.results;
  const seen = new Set<string>();
  const results = list.map((r, i) => {
    if (!isRecord(r)) bad(`report: result ${String(i)} is not an object`);
    const { site, status, reason } = r;
    if (typeof site !== "string" || site.length === 0 || site.length > 253)
      bad(`report: result ${String(i)} has a bad site`);
    if (seen.has(site)) bad(`report: ${site} appears twice`);
    seen.add(site);
    if (!isStatus(status)) bad(`report: ${site} has a bad status`);
    if (
      reason !== undefined &&
      (typeof reason !== "string" || reason.length > 200)
    )
      bad(`report: ${site} has a bad reason`);
    return reason === undefined ? { site, status } : { site, status, reason };
  });
  const counted = totals(results.map((r) => r.status));
  const given = input.totals;
  if (
    !isRecord(given) ||
    (Object.keys(counted) as (keyof typeof counted)[]).some(
      (k) => given[k] !== counted[k],
    )
  )
    bad("report: the totals don't match the sites");
  return { date, software, results, totals: counted };
}

/** Checks a downloaded history.json (v1). */
export function checkHistory(input: unknown): History {
  if (!isRecord(input)) bad("history: not an object");
  if (input.v !== 1) bad("history: not version 1");
  if (!Array.isArray(input.nights)) bad("history: no nights");
  const list: unknown[] = input.nights;
  let last = "";
  const nights = list.map((n, i): Night => {
    if (!isRecord(n)) bad(`history: night ${String(i)} is not an object`);
    const { date, pass_rate, sites } = n;
    if (typeof date !== "string" || !DAY.test(date) || date <= last)
      bad(`history: night ${String(i)} has a bad date`);
    last = date;
    if (
      typeof pass_rate !== "number" ||
      !Number.isFinite(pass_rate) ||
      pass_rate < 0 ||
      pass_rate > 100
    )
      bad(`history: ${date} has a bad pass rate`);
    if (!isRecord(sites)) bad(`history: ${date} has no sites`);
    const out: Record<string, Status> = {};
    for (const [site, status] of Object.entries(sites)) {
      if (!isStatus(status)) bad(`history: ${date} has a bad status`);
      out[site] = status;
    }
    return { date, pass_rate, sites: out };
  });
  return { v: 1, nights };
}

/** The run's day in UTC. */
export const dayOf = (report: CheckedReport): string =>
  new Date(report.date).toISOString().slice(0, 10);

/**
 * Adds the report's night, replacing one from the same day (a run started
 * by hand), and keeps the last NIGHTS_KEPT nights.
 */
export function addNight(history: History, report: CheckedReport): History {
  const date = dayOf(report);
  const before = history.nights.filter((n) => n.date !== date);
  const latest = before.at(-1);
  if (latest !== undefined && latest.date > date)
    bad(`history: already has ${latest.date}, after ${date}`);
  const sites: Record<string, Status> = {};
  for (const r of report.results) sites[r.site] = r.status;
  const night: Night = { date, pass_rate: report.totals.pass_rate, sites };
  return { v: 1, nights: [...before, night].slice(-NIGHTS_KEPT) };
}

/** Whole days from one `YYYY-MM-DD` to another. */
export function daysBetween(from: string, to: string): number {
  return Math.round(
    (Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) /
      86_400_000,
  );
}

/**
 * The first of the consecutive nights, ending with the last one, on which
 * the site had its current status. A site new to the list counts from its
 * first night.
 */
export function since(nights: Night[], site: string): string {
  const last = nights.at(-1);
  if (last === undefined) bad("history: no nights");
  const status = last.sites[site];
  let i = nights.length - 1;
  while (i > 0 && status !== undefined && nights[i - 1]?.sites[site] === status)
    i--;
  return nights[i]?.date ?? last.date;
}

/** Sites that don't work first, then the rest by name. */
export function rows(report: CheckedReport, history: History): Row[] {
  const today = dayOf(report);
  return report.results
    .map((r): Row => {
      const from = since(history.nights, r.site);
      const row: Row = {
        site: r.site,
        status: r.status,
        since: from,
        over_sla: r.status === "fail" && daysBetween(from, today) > SLA_DAYS,
      };
      if (r.reason !== undefined) row.reason = r.reason;
      return row;
    })
    .sort(
      (a, b) =>
        Number(b.status === "fail") - Number(a.status === "fail") ||
        (a.site < b.site ? -1 : a.site > b.site ? 1 : 0),
    );
}

/**
 * The night says more about our runner than about the sites: too many
 * pages ended as `error`, or there were no sites at all. Bot checks
 * (`blocked`) don't count.
 */
export function badNight(report: CheckedReport): boolean {
  const n = report.results.length;
  return n === 0 || report.totals.error / n > MAX_ERROR_SHARE;
}

const REASONS: Record<string, string> = {
  wrong_inputs: "Filled the wrong fields",
  no_form: "Found no login form",
  field_hidden: "A field looked hidden, so nothing was filled",
  refused: "Wouldn't fill on this page",
  page_changed: "The page changed while filling",
  needs_confirm: "Asked for a confirmation first",
  no_page: "Couldn't read the page",
  "no tab": "Couldn't read the page",
  challenge: "The site showed a bot check",
  bot_wall: "The site blocked automated visits",
  timeout: "The page didn't load in 20 seconds",
  navigation: "The page couldn't be opened",
  runner: "Our test setup failed",
  locked: "Our test setup failed",
  unknown_item: "Our test setup failed",
};

/** The runner's reason code in plain words; an unknown one is "Other". */
export function reasonText(code: string | undefined): string {
  if (code === undefined || code === "") return "";
  const known = REASONS[code];
  if (known !== undefined) return known;
  if (code.startsWith("field_hidden:")) return REASONS.field_hidden ?? "";
  const http = /^http_(\d{3})$/.exec(code);
  if (http) return `The site answered with an error page (${http[1] ?? ""})`;
  if (code.startsWith("net::ERR_")) return "The page couldn't be reached";
  return "Other";
}

const ESCAPES: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
};

/** Text for HTML content and attribute values. */
export const esc = (s: string): string =>
  s.replace(/[&<>"']/g, (c) => ESCAPES[c] ?? c);

const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

/** `2026-11-08` as "8 Nov 2026", the same on every machine. */
export function showDay(day: string): string {
  const [y, m, d] = day.split("-");
  return `${String(Number(d))} ${MONTHS[Number(m) - 1] ?? ""} ${y ?? ""}`;
}

const time = (day: string): string =>
  `<time datetime="${esc(day)}">${esc(showDay(day))}</time>`;

const percent = (n: number): string => `${String(n)}%`;

/** Sites tested a night: those counted in its pass rate. */
const tested = (n: Night): number =>
  Object.values(n.sites).filter((s) => s === "pass" || s === "fail").length;

/** The pass rate over the nights as an inline SVG, styled by style.css. */
export function trend(nights: Night[]): string {
  const w = 640;
  const h = 160;
  const left = 60;
  const right = 12;
  const top = 12;
  const bottom = 28;
  const lowest = Math.min(...nights.map((n) => n.pass_rate), TARGET);
  const floor = Math.max(0, Math.floor((lowest - 5) / 10) * 10);
  const x = (i: number) =>
    nights.length === 1
      ? left + (w - left - right) / 2
      : left + (i * (w - left - right)) / (nights.length - 1);
  const y = (rate: number) =>
    top + ((100 - rate) * (h - top - bottom)) / (100 - floor);
  const r1 = (n: number) => String(Math.round(n * 10) / 10);
  const first = nights[0];
  const last = nights.at(-1);
  if (first === undefined || last === undefined) return "";
  const label =
    nights.length === 1
      ? `Pass rate ${percent(last.pass_rate)} on ${showDay(last.date)}.`
      : `Pass rate from ${percent(first.pass_rate)} on ${showDay(first.date)} to ${percent(last.pass_rate)} on ${showDay(last.date)}.`;
  const grid = [floor, TARGET, LAUNCH_TARGET, 100]
    .filter((v, i, all) => all.indexOf(v) === i)
    .map((v) => {
      const cls = v === TARGET || v === LAUNCH_TARGET ? "grid target" : "grid";
      return `<line class="${cls}" x1="${String(left)}" x2="${String(w - right)}" y1="${r1(y(v))}" y2="${r1(y(v))}"/><text class="axis" x="${String(left - 8)}" y="${r1(y(v) + 4)}" text-anchor="end">${String(v)}%</text>`;
    })
    .join("");
  const points = nights
    .map((n, i) => `${r1(x(i))},${r1(y(n.pass_rate))}`)
    .join(" ");
  const line =
    nights.length === 1
      ? ""
      : `<polyline class="line" points="${points}" fill="none"/>`;
  const dot = `<circle class="dot" cx="${r1(x(nights.length - 1))}" cy="${r1(y(last.pass_rate))}" r="4"/>`;
  const dates = `<text class="axis" x="${String(left)}" y="${String(h - 8)}">${esc(showDay(first.date))}</text>${
    nights.length === 1
      ? ""
      : `<text class="axis" x="${String(w - right)}" y="${String(h - 8)}" text-anchor="end">${esc(showDay(last.date))}</text>`
  }`;
  return `<svg class="trend" viewBox="0 0 ${String(w)} ${String(h)}" role="img" aria-labelledby="trend-title"><title id="trend-title">${esc(label)}</title>${grid}${line}${dot}${dates}</svg>`;
}

function page(
  report: CheckedReport,
  history: History,
  options: BuildOptions,
): string {
  const today = dayOf(report);
  const t = report.totals;
  const list = rows(report, history);
  const versions = Object.entries(report.software)
    .map(([k, v]) =>
      k === "browser"
        ? `Chromium ${esc(v)}`
        : k === "extension"
          ? `extension ${esc(v)}`
          : `${esc(k)} ${esc(v)}`,
    )
    .join(", ");
  const counts = STATUSES.map(
    (s) =>
      `<div class="count count-${s}"><dt>${esc(STATUS_TEXT[s])}</dt><dd>${String(t[s])}</dd></div>`,
  ).join("");
  const body = list
    .map((r) => {
      const sla = r.over_sla
        ? ` <span class="sla">Over ${String(SLA_DAYS)} days</span>`
        : "";
      return `<tr class="row-${r.status}"><th scope="row" data-label="Site">${esc(r.site)}</th><td data-label="Status"><span class="status status-${r.status}">${esc(STATUS_TEXT[r.status])}</span>${sla}</td><td data-label="Since">${time(r.since)}</td><td data-label="Why">${esc(reasonText(r.reason))}</td></tr>`;
    })
    .join("\n");
  const numbers = [...history.nights]
    .reverse()
    .map(
      (n) =>
        `<tr><th scope="row">${time(n.date)}</th><td>${percent(n.pass_rate)}</td><td>${String(tested(n))}</td></tr>`,
    )
    .join("\n");
  const name = esc(PRODUCT);
  const tranco = options.tranco
    ? `<li>Sites are ordered by the Tranco list (CC BY 4.0).</li>`
    : "";
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'self'; img-src 'self'">
<meta name="referrer" content="no-referrer">
<meta name="description" content="Every night we check whether ${name} fills the login form on popular sites.">
<title>${name} autofill compatibility</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
<header class="masthead">
<p class="brand">${name}</p>
<h1>Autofill compatibility</h1>
<p class="lede">Every night we check whether ${name} fills the login form on popular sites. These are the latest results.</p>
</header>
<main>
<section class="summary" aria-labelledby="summary-title">
<h2 id="summary-title">Last night</h2>
<div class="rate">
<p class="rate-number">${percent(t.pass_rate)}</p>
<p class="rate-text">of the sites we could test filled correctly on ${time(today)}.</p>
<p class="rate-target">Target: ${percent(TARGET)} now, ${percent(LAUNCH_TARGET)} by launch.</p>
</div>
<dl class="counts">${counts}</dl>
</section>
<section class="history" aria-labelledby="history-title">
<h2 id="history-title">Last ${String(NIGHTS_KEPT)} nights</h2>
${trend(history.nights)}
<details>
<summary>Show the numbers</summary>
<table class="numbers">
<thead><tr><th scope="col">Night</th><th scope="col">Pass rate</th><th scope="col">Sites tested</th></tr></thead>
<tbody>
${numbers}
</tbody>
</table>
</details>
</section>
<section class="sites" aria-labelledby="sites-title">
<h2 id="sites-title">Sites</h2>
<p class="note">Sites that don't work come first. A site marked "Over ${String(SLA_DAYS)} days" has gone longer than our fix target.</p>
<table class="site-table">
<thead><tr><th scope="col">Site</th><th scope="col">Status</th><th scope="col">Since</th><th scope="col">Why</th></tr></thead>
<tbody>
${body}
</tbody>
</table>
</section>
<section class="how" aria-labelledby="how-title">
<h2 id="how-title">How we test</h2>
<ul>
<li>Each night, Chromium with the ${name} extension built that day opens each site's login page, one site at a time.</li>
<li>It fills a made-up login and never submits it. No real account is used.</li>
<li>A site works when the username and password reach the right fields. On sites that ask for the username first, the first page counts.</li>
<li>Bot checks and error pages are left alone. Those sites show as "Not tested" and don't count towards the pass rate.</li>
<li>The pass rate is the sites that work out of the sites we could test.</li>
${tranco}
</ul>
<p class="versions">Last run: ${versions === "" ? "versions not recorded" : versions}.</p>
</section>
</main>
<footer class="footer">
<p>The data is in <a href="report.json">report.json</a> and <a href="history.json">history.json</a>.</p>
</footer>
</body>
</html>
`;
}

type Tokens = {
  color: Record<"light" | "dark", Record<string, { $value: string }>>;
  font: { family: Record<string, { $value: string[] }> };
};

/** The page's CSS, with colors and fonts from design/tokens.json. */
export function css(tokens: Tokens): string {
  const vars = (mode: "light" | "dark") =>
    Object.entries(tokens.color[mode])
      .map(([k, v]) => `  --${k}: ${v.$value};`)
      .join("\n");
  const family = (k: string) =>
    (tokens.font.family[k]?.$value ?? [])
      .map((f) => (/\s/.test(f) ? `"${f}"` : f))
      .join(", ");
  return `/* The autofill dashboard (D77). Colors from design/tokens.json. */
:root {
${vars("light")}
  color-scheme: light dark;
}
@media (prefers-color-scheme: dark) {
  :root {
${vars("dark")
  .split("\n")
  .map((l) => `  ${l}`)
  .join("\n")}
  }
}
* { box-sizing: border-box; }
html { -webkit-text-size-adjust: 100%; }
body {
  margin: 0;
  background: var(--bg);
  color: var(--text);
  font: 400 15px/1.5 ${family("sans")};
}
a { color: var(--accent); }
a:focus-visible, summary:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; border-radius: 4px; }
.masthead, main, .footer { max-width: 920px; margin: 0 auto; padding: 0 16px; }
.masthead { padding-top: 48px; padding-bottom: 8px; }
.brand { margin: 0 0 8px; color: var(--accent); font-weight: 600; letter-spacing: 0.02em; }
h1 { margin: 0; font-size: 28px; line-height: 1.21; font-weight: 600; }
.lede { margin: 12px 0 0; color: var(--text-muted); max-width: 60ch; }
h2 { margin: 0 0 16px; font-size: 17px; line-height: 1.29; font-weight: 600; }
section { margin-top: 32px; }
.summary {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1.2fr);
  gap: 24px;
  align-items: center;
  padding: 24px;
  background: var(--surface);
  border-radius: 20px;
}
.summary h2 { grid-column: 1 / -1; margin: 0; color: var(--text-muted); font-size: 14px; font-weight: 500; }
.rate p { margin: 0; }
.rate-number { font-size: 56px; line-height: 1; font-weight: 600; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; }
.rate .rate-text { margin-top: 8px; }
.rate .rate-target { margin-top: 4px; color: var(--text-muted); font-size: 13px; }
.counts { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin: 0; }
.count { padding: 12px 16px; background: var(--bg); border-radius: 16px; }
.count dt { color: var(--text-muted); font-size: 13px; }
.count dd { margin: 2px 0 0; font-size: 22px; font-weight: 600; font-variant-numeric: tabular-nums; }
.count-pass dd { color: var(--success); }
.count-fail dd { color: var(--danger); }
.trend { display: block; width: 100%; height: auto; }
.trend .grid { stroke: var(--border); stroke-width: 1; }
.trend .target { stroke: var(--text-muted); stroke-dasharray: 4 4; }
.trend .axis { fill: var(--text-muted); font-size: 12px; font-family: ${family("sans")}; }
.trend .line { stroke: var(--accent); stroke-width: 2.5; stroke-linejoin: round; stroke-linecap: round; }
.trend .dot { fill: var(--accent); }
details { margin-top: 12px; }
summary { cursor: pointer; color: var(--accent); }
table { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
th, td { padding: 10px 12px; text-align: left; vertical-align: top; border-bottom: 1px solid var(--border); }
thead th { color: var(--text-muted); font-size: 13px; font-weight: 500; }
tbody th { font-weight: 600; overflow-wrap: anywhere; }
.numbers { margin-top: 12px; max-width: 420px; }
.note { margin: -8px 0 16px; color: var(--text-muted); }
.status { font-weight: 600; white-space: nowrap; }
.status::before { content: ""; display: inline-block; width: 8px; height: 8px; margin-right: 8px; border-radius: 50%; background: currentColor; vertical-align: 1px; }
.status-pass { color: var(--success); }
.status-fail { color: var(--danger); }
.status-blocked { color: var(--warning); }
.status-error { color: var(--text-muted); }
.sla { display: inline-block; margin-left: 8px; padding: 0 8px; border-radius: 999px; background: var(--accent-soft); color: var(--accent); font-size: 12px; font-weight: 600; white-space: nowrap; }
.how ul { margin: 0; padding-left: 20px; max-width: 70ch; }
.how li + li { margin-top: 6px; }
.versions { color: var(--text-muted); font-size: 13px; }
.footer { margin-top: 48px; padding-bottom: 48px; color: var(--text-muted); font-size: 13px; }
@media (max-width: 640px) {
  .masthead { padding-top: 32px; }
  .summary { grid-template-columns: minmax(0, 1fr); padding: 16px; }
  .rate-number { font-size: 44px; }
  .trend .axis { font-size: 20px; }
  .trend .line { stroke-width: 4; }
  .trend .dot { r: 6; }
  .site-table thead { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
  .site-table tr { display: block; padding: 12px 0; border-bottom: 1px solid var(--border); }
  .site-table th, .site-table td { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 12px; padding: 2px 0; border: 0; }
  .site-table th::before, .site-table td::before { content: attr(data-label); flex: 0 0 4.5em; color: var(--text-muted); font-size: 13px; font-weight: 400; }
  .site-table td:empty { display: none; }
  .sla { margin-left: 0; }
}
`;
}

/** Every file of the site, by name. */
export function build(
  report: CheckedReport,
  previous: History,
  tokens: Tokens,
  options: BuildOptions = {},
): Record<string, string> {
  const history = addNight(previous, report);
  const published = {
    v: 1,
    date: report.date,
    software: report.software,
    totals: report.totals,
    sites: rows(report, history).map((r) =>
      r.reason === undefined
        ? { site: r.site, status: r.status, since: r.since }
        : { site: r.site, status: r.status, reason: r.reason, since: r.since },
    ),
  };
  return {
    "index.html": page(report, history, options),
    "style.css": css(tokens),
    "report.json": JSON.stringify(published, null, 2) + "\n",
    "history.json": JSON.stringify(history) + "\n",
  };
}

function readJson(file: string): unknown {
  return JSON.parse(fs.readFileSync(path.resolve(file), "utf8"));
}

function main(): void {
  const arg = (name: string): string | undefined => {
    const i = process.argv.indexOf(`--${name}`);
    return i > 0 ? process.argv[i + 1] : undefined;
  };
  const flag = (name: string) => process.argv.includes(`--${name}`);
  const reportFile = arg("report");
  const historyFile = arg("history");
  const out = arg("out");
  const first = flag("first");
  if (
    reportFile === undefined ||
    out === undefined ||
    (historyFile === undefined) === !first
  ) {
    console.error(
      "usage: node dashboard/build.ts --report <file> (--history <file> | --first) --out <dir> [--tranco]",
    );
    process.exit(2);
  }
  try {
    const report = checkReport(readJson(reportFile));
    if (badNight(report)) {
      console.error(
        `not published: ${String(report.totals.error)} of ${String(report.results.length)} sites couldn't load, more than ${String(MAX_ERROR_SHARE * 100)}%; the runner probably broke`,
      );
      process.exit(1);
    }
    const history =
      historyFile === undefined
        ? { v: 1 as const, nights: [] }
        : checkHistory(readJson(historyFile));
    const tokens = readJson(
      path.resolve(import.meta.dirname, "../../../design/tokens.json"),
    ) as Tokens;
    const files = build(report, history, tokens, { tranco: flag("tranco") });
    fs.mkdirSync(out, { recursive: true });
    for (const [name, text] of Object.entries(files))
      fs.writeFileSync(path.join(out, name), text);
    console.log(
      `built ${out}: ${String(report.totals.pass_rate)}% on ${dayOf(report)}`,
    );
  } catch (error) {
    if (error instanceof BadInput || error instanceof SyntaxError) {
      console.error(`not published: ${error.message}`);
      process.exit(1);
    }
    throw error;
  }
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === path.resolve(import.meta.filename)
) {
  main();
}

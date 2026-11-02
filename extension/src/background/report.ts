/**
 * "Autofill didn't work here" reports (D38): the core builds the report
 * from the page's field descriptions (never values); the user sees it, and
 * only Send posts exactly that, anonymously, to the hosted service.
 */
import { HOSTED_SERVER } from "../shared/config.ts";
import type { ReportPreview } from "../shared/messages.ts";
import { EXTENSION_VERSION } from "../version.ts";
import type { Core } from "./core.ts";
import { ServiceError } from "./errors.ts";
import type { Autofill } from "./fill.ts";
import { iconFields } from "./plan.ts";
import type { Platform } from "./platform.ts";

export const REPORT_PATH = "/v1/reports/autofill";

export interface ReporterHost {
  core(): Core;
  platform: Platform;
  autofill: Autofill;
}

/** The last two host labels; enough to say whether a frame is the page's site. */
function site(url: string): string {
  try {
    return new URL(url).hostname.split(".").slice(-2).join(".");
  } catch {
    return "";
  }
}

interface ReportJson {
  site: string;
  frames: { fields: { form: number | null; class: string | null }[] }[];
}

export class Reporter {
  /** The report the user looked at, per tab: Send posts exactly this. */
  private previews = new Map<number, string>();

  constructor(private readonly host: ReporterHost) {}

  async preview(problem: string): Promise<ReportPreview> {
    const tab = await this.host.platform.pages.activeTab();
    if (!tab || !/^https?:/.test(tab.url)) throw new ServiceError("no_page");
    const frames = await this.host.autofill.scanTab(tab.id);
    const framesJson = JSON.stringify(
      frames.map((f) => ({
        relation:
          f.ref.frameId === 0
            ? "top"
            : site(f.url) === site(f.tabUrl)
              ? "same_site"
              : "other_site",
        fields: f.fields,
        fill: iconFields(f.fields, f.classes),
      })),
    );
    const json = this.host
      .core()
      .autofillReport(
        tab.url,
        problem,
        framesJson,
        EXTENSION_VERSION,
        this.host.platform.browser,
      );
    this.previews.set(tab.id, json);
    const report = JSON.parse(json) as ReportJson;
    const fields = report.frames.flatMap((f) => f.fields);
    const forms = new Set(
      report.frames.flatMap((f, i) =>
        f.fields.map((x) => `${String(i)}:${String(x.form)}`),
      ),
    );
    const kinds = [...new Set(fields.map((f) => f.class ?? "unknown"))].sort();
    return {
      site: report.site,
      forms: forms.size,
      fields: fields.length,
      kinds,
      json,
    };
  }

  /** Posts the previewed report: no token, no cookies, nothing else. */
  async send(): Promise<null> {
    const tab = await this.host.platform.pages.activeTab();
    const json = tab ? this.previews.get(tab.id) : undefined;
    if (!tab || json === undefined) throw new ServiceError("no_report");
    let response: Response;
    try {
      response = await this.host.platform.fetch(HOSTED_SERVER + REPORT_PATH, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: json,
        credentials: "omit",
        cache: "no-store",
        redirect: "error",
      });
    } catch {
      throw new ServiceError("network");
    }
    if (response.status === 429) throw new ServiceError("rate_limited");
    if (response.status !== 202) throw new ServiceError("server");
    this.previews.delete(tab.id);
    return null;
  }

  onTabRemoved(tabId: number): void {
    this.previews.delete(tabId);
  }
}

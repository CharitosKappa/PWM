/**
 * The manage page's requests (D44): the account's devices, the health
 * report with its leak check, and export. Leak results stay in memory until
 * the vault locks; an export is checked and made in one request, so a
 * stopped service worker never leaves a grant behind.
 */
import type {
  DeviceView,
  ExportFormat,
  HealthItem,
  HealthView,
} from "../shared/messages.ts";
import { toBase64 } from "../shared/bytes.ts";
import type { Core, CoreFunctions, CoreStep } from "./core.ts";
import { errorCode } from "./core.ts";
import { ServiceError } from "./errors.ts";
import { localDate } from "./kit.ts";
import type { Platform } from "./platform.ts";

/** The origin the leak check needs, asked for by the page (D44 §2). */
export const LEAK_CHECK_ORIGIN = "https://api.pwnedpasswords.com/*";
const FORMATS: readonly ExportFormat[] = ["pwmx", "json", "csv"];

interface ManagerDeps {
  core: () => Core;
  platform: Platform;
  fns: CoreFunctions;
  /** A server conversation, queued with the others; its last step. */
  run: (start: () => CoreStep) => Promise<CoreStep>;
  /** The end of a run that is not this request's own outcome (revoked, signed out, offline). */
  ended: (step: CoreStep) => Promise<void>;
  secretKey: () => Promise<string | undefined>;
  touch: () => Promise<void>;
}

interface DeviceJson {
  id: string;
  platform: string;
  name: string | null;
  created_at: number;
  last_seen_at: number;
  revoked_at: number | null;
  current: boolean;
}

export class Manager {
  private leaked: Map<string, number> | null = null;

  constructor(private readonly deps: ManagerDeps) {}

  /** Forgets leak results; called when the vault locks. */
  clear(): void {
    this.leaked = null;
  }

  // --- Devices ---

  async devices(): Promise<DeviceView[]> {
    await this.deps.touch();
    const step = await this.conversation(
      () => this.deps.core().startListDevices(),
      "devices",
    );
    const list = JSON.parse(step.devices ?? "[]") as DeviceJson[];
    return list.map((d) => ({
      id: d.id,
      platform: d.platform,
      name: d.name,
      createdAt: d.created_at,
      lastSeenAt: d.last_seen_at,
      revokedAt: d.revoked_at,
      current: d.current,
    }));
  }

  async rename(deviceId: string, name: string): Promise<DeviceView[]> {
    const trimmed = name.trim();
    if (trimmed === "") throw new ServiceError("invalid_device_name");
    await this.conversation(
      () => this.deps.core().startRenameDevice(deviceId, trimmed),
      "device_updated",
    );
    return this.devices();
  }

  /** Another device only: this one leaves with Sign out. */
  async revoke(deviceId: string): Promise<DeviceView[]> {
    const current = (await this.devices()).find((d) => d.current);
    if (current?.id === deviceId) throw new ServiceError("this_device");
    await this.conversation(
      () => this.deps.core().startRevokeDevice(deviceId),
      "device_updated",
    );
    return this.devices();
  }

  private async conversation(
    start: () => CoreStep,
    outcome: string,
  ): Promise<CoreStep> {
    const step = await this.deps.run(start);
    if (step.kind === "done" && step.outcome === outcome) return step;
    await this.deps.ended(step);
    if (step.kind === "wait") throw new ServiceError("network");
    throw new ServiceError(step.outcome ?? "internal");
  }

  // --- Health ---

  async health(): Promise<HealthView> {
    await this.deps.touch();
    const items = this.loginsById();
    const report = JSON.parse(
      this.call(() => this.deps.core().healthReport()),
    ) as {
      weak: { item_id: string }[];
      reused: string[][];
    };
    const named = (ids: string[]): HealthItem[] =>
      ids.flatMap((id) => {
        const item = items.get(id);
        return item ? [item] : [];
      });
    const byTitle = (a: HealthItem, b: HealthItem): number =>
      a.title.localeCompare(b.title) || a.itemId.localeCompare(b.itemId);
    return {
      weak: named(report.weak.map((w) => w.item_id)).sort(byTitle),
      reused: report.reused
        .map((group) => named(group).sort(byTitle))
        .filter((group) => group.length > 1),
      leaked:
        this.leaked === null
          ? null
          : [...this.leaked]
              .flatMap(([id, count]) => {
                const item = items.get(id);
                return item ? [{ ...item, count }] : [];
              })
              .sort(byTitle),
      leakCheck: await this.leakCheckOn(),
    };
  }

  async setLeakCheck(on: boolean): Promise<boolean> {
    await this.deps.platform.disk.set("leakCheck", on ? "on" : "off");
    if (!on) this.leaked = null;
    return on;
  }

  /**
   * The leak check (D22 §4): only 5-character hash prefixes leave, one
   * request each, straight to HIBP with padding and nothing that identifies
   * the account.
   */
  async checkLeaks(): Promise<HealthView> {
    if (!(await this.leakCheckOn())) throw new ServiceError("leak_check_off");
    if (!(await this.deps.platform.pages.hasHostAccess(LEAK_CHECK_ORIGIN)))
      throw new ServiceError("no_leak_access");
    const check = this.call(() => this.deps.core().breachCheck());
    for (const prefix of check.prefixes()) {
      let response: Response;
      try {
        response = await this.deps.platform.fetch(
          this.deps.fns.breachRangeUrl(prefix),
          {
            method: "GET",
            headers: { "Add-Padding": "true" },
            credentials: "omit",
            cache: "no-store",
            redirect: "error",
            referrerPolicy: "no-referrer",
          },
        );
      } catch {
        throw new ServiceError("network");
      }
      if (response.status !== 200) throw new ServiceError("breach_check");
      const body = await response.text();
      this.call(() => {
        check.addResponse(prefix, body);
      });
    }
    const breached = JSON.parse(check.finish()) as {
      item_id: string;
      count: number;
    }[];
    this.leaked = new Map(breached.map((b) => [b.item_id, b.count]));
    return this.health();
  }

  private async leakCheckOn(): Promise<boolean> {
    return (await this.deps.platform.disk.get("leakCheck")) !== "off";
  }

  /** Every login not in the trash, by id. */
  private loginsById(): Map<string, HealthItem> {
    const core = this.deps.core();
    const out = new Map<string, HealthItem>();
    for (const vaultId of this.call(() => core.vaultIds())) {
      for (const text of this.call(() => core.items(vaultId))) {
        const item = JSON.parse(text) as {
          id?: unknown;
          type?: unknown;
          title?: unknown;
          trashed_at?: unknown;
          login?: { username?: unknown; urls?: { url?: unknown }[] };
        };
        if (item.type !== "login" || typeof item.id !== "string") continue;
        if (item.trashed_at !== undefined && item.trashed_at !== null) continue;
        const url = item.login?.urls?.[0]?.url;
        out.set(item.id, {
          vaultId,
          itemId: item.id,
          title: typeof item.title === "string" ? item.title : "",
          username:
            typeof item.login?.username === "string" ? item.login.username : "",
          url: typeof url === "string" ? url : "",
        });
      }
    }
    return out;
  }

  // --- Export ---

  /**
   * Checks the master password and makes the file in one go (D44 §6). A
   * weak export password is refused first, so no grant is ever left unused.
   */
  async exportVault(
    masterPassword: string,
    format: ExportFormat,
    exportPassword: string,
  ): Promise<{ file: string; name: string }> {
    if (!FORMATS.includes(format)) throw new ServiceError("invalid_request");
    if (
      format === "pwmx" &&
      this.deps.fns.passwordScore(exportPassword, []) < 3
    ) {
      throw new ServiceError("weak_export_password");
    }
    const secretKey = await this.deps.secretKey();
    if (secretKey === undefined) throw new ServiceError("signed_out");
    const core = this.deps.core();
    const now = this.deps.platform.now();
    this.call(() => {
      core.authorizeExport(masterPassword, secretKey, now);
    });
    const file = this.call(() =>
      core.exportFile(format, format === "pwmx" ? exportPassword : "", now),
    );
    await this.deps.touch();
    return {
      file: toBase64(file),
      name: `PWM export ${localDate(now)}.${format}`,
    };
  }

  /** A core call whose error becomes a ServiceError with its code. */
  private call<T>(fn: () => T): T {
    try {
      return fn();
    } catch (error) {
      throw new ServiceError(errorCode(error));
    }
  }
}

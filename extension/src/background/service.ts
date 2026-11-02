/**
 * The background service (D34): owns the core, the cache, the session and
 * every server conversation, and answers the popup. Browser APIs come in
 * through `Platform`, so this is tested with fakes.
 */
import { AUTO_LOCK_CHOICES } from "../shared/messages.ts";
import type {
  ItemSummary,
  Replies,
  Request,
  Settings,
  Status,
} from "../shared/messages.ts";
import { fromBase64, toBase64 } from "../shared/bytes.ts";
import { HOSTED_SERVER } from "../shared/config.ts";
import { errorCode } from "./core.ts";
import type { Core, CoreFunctions, CoreStep } from "./core.ts";
import { runSteps } from "./http.ts";
import type { AlarmName, Platform } from "./platform.ts";

export const RULES_PATH = "/autofill-rules.bin";
const CLIPBOARD_CLEAR_MS = 30_000;
const RULES_EVERY_MS = 24 * 60 * 60 * 1000;
const RULES_SPREAD_MS = 2 * 60 * 60 * 1000;
const DEVICE_NAME = "Browser extension";
const DEFAULT_SETTINGS: Settings = {
  serverUrl: HOSTED_SERVER,
  autoLockMinutes: 15,
};

export class ServiceError extends Error {
  constructor(readonly code: string) {
    super(code);
  }
}

interface StoredItem {
  id?: unknown;
  type?: unknown;
  title?: unknown;
  trashed_at?: unknown;
  username?: unknown;
  password?: unknown;
  totp?: unknown;
  urls?: unknown;
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function firstUrl(urls: unknown): string {
  if (!Array.isArray(urls)) return "";
  const first: unknown = urls[0];
  return typeof first === "object" && first !== null && "url" in first
    ? text(first.url)
    : "";
}

export class Background {
  private core: Core;
  private settings: Settings = DEFAULT_SETTINGS;
  private syncedAt: number | null = null;
  private syncError: string | null = null;
  private syncing: Promise<Status> | null = null;
  private queue: Promise<unknown> = Promise.resolve();

  private constructor(
    private readonly fns: CoreFunctions,
    private readonly platform: Platform,
    core: Core,
  ) {
    this.core = core;
  }

  /** Loads the cache and settings, and restores a session after a worker restart. */
  static async start(
    fns: CoreFunctions,
    platform: Platform,
  ): Promise<Background> {
    const snapshot = await platform.disk.getSnapshot();
    let core: Core;
    try {
      core = snapshot ? fns.loadCore(snapshot) : fns.newCore();
    } catch {
      core = fns.newCore();
    }
    const background = new Background(fns, platform, core);
    const stored = await platform.disk.get("settings");
    if (stored) background.settings = parseSettings(stored);
    await background.loadRules();
    await background.restoreSession();
    return background;
  }

  /** The core keeps rules in memory: give it the last accepted file again. */
  private async loadRules(): Promise<void> {
    const rules = await this.platform.disk.get("rules");
    if (rules === undefined) return;
    const version = Number(
      (await this.platform.disk.get("rulesVersion")) ?? "0",
    );
    try {
      this.core.setAutofillRules(
        fromBase64(rules),
        this.platform.now(),
        version,
      );
    } catch {
      // Expired or no longer trusted: the built-in rules apply until the next download.
    }
  }

  private async restoreSession(): Promise<void> {
    const session = await this.platform.session.get();
    if (!session || this.core.accountId() === undefined) return;
    try {
      this.core.restoreSession(session, await this.sealedTokens());
      await this.touch();
    } catch {
      await this.platform.session.clear();
    }
  }

  private async sealedTokens(): Promise<Uint8Array | undefined> {
    const sealed = await this.platform.disk.get("sealedTokens");
    return sealed ? fromBase64(sealed) : undefined;
  }

  /** Answers one popup request; every failure is a ServiceError with a code. */
  async handle<R extends Request>(request: R): Promise<Replies[R["kind"]]> {
    try {
      // dispatch answers each kind with its own reply type.
      return (await this.dispatch(request)) as Replies[R["kind"]];
    } catch (error) {
      throw error instanceof ServiceError
        ? error
        : new ServiceError(errorCode(error));
    }
  }

  private async dispatch(request: Request): Promise<Replies[Request["kind"]]> {
    switch (request.kind) {
      case "status":
        return this.status();
      case "signUp":
        return this.signUp(request.email, request.password);
      case "signIn":
        await this.signIn(request.email, request.password, request.secretKey);
        return null;
      case "unlock":
        await this.unlock(request.password);
        return null;
      case "lock":
        await this.lock();
        return null;
      case "signOut":
        await this.signOut();
        return null;
      case "items":
        await this.touch();
        return this.items(request.query);
      case "copy":
        await this.touch();
        await this.copy(request.vaultId, request.itemId, request.field);
        return null;
      case "copySecretKey": {
        const secretKey = await this.platform.disk.get("secretKey");
        if (secretKey === undefined) throw new ServiceError("signed_out");
        await this.writeClipboard(secretKey);
        return null;
      }
      case "copyText":
        await this.touch();
        await this.writeClipboard(request.text);
        return null;
      case "generate":
        await this.touch();
        return request.mode === "password"
          ? this.fns.generatePassword(20)
          : this.fns.generatePassphrase(5);
      case "score":
        return this.fns.passwordScore(request.password, [request.email]);
      case "sync":
        return this.sync();
      case "settings":
        return this.settings;
      case "setSettings":
        return this.setSettings(request.settings);
      case "activity":
        await this.touch();
        return null;
    }
  }

  async status(): Promise<Status> {
    if (this.core.accountId() === undefined) return { state: "signedOut" };
    const email = (await this.platform.disk.get("email")) ?? "";
    if (!this.core.isUnlocked()) return { state: "locked", email };
    return {
      state: "unlocked",
      email,
      syncedAt: this.syncedAt,
      syncError: this.syncError,
    };
  }

  private async deviceId(): Promise<string> {
    const stored = await this.platform.disk.get("deviceId");
    if (stored) return stored;
    const id = this.fns.newId();
    await this.platform.disk.set("deviceId", id);
    return id;
  }

  /**
   * Runs a server conversation; the core has one at a time, so they queue.
   * Core errors become ServiceErrors with their code.
   */
  private run(start: () => CoreStep): Promise<CoreStep> {
    const next = this.queue.then(() => this.runNow(start));
    this.queue = next.catch(() => undefined);
    return next;
  }

  private async runNow(start: () => CoreStep): Promise<CoreStep> {
    let first: CoreStep;
    try {
      first = start();
    } catch (error) {
      throw new ServiceError(errorCode(error));
    }
    let last: CoreStep;
    try {
      last = await runSteps(
        this.core,
        first,
        this.settings.serverUrl,
        this.platform.fetch,
        this.platform.now,
      );
    } catch (error) {
      this.core.cancel();
      throw new ServiceError(errorCode(error));
    }
    await this.saveTokens();
    await this.persist();
    return last;
  }

  /** Seals any new tokens the core handed out and stores them. */
  private async saveTokens(): Promise<void> {
    const tokens = this.core.takeTokens();
    if (!tokens || !this.core.isUnlocked()) return;
    const [access, refresh] = tokens;
    if (access === undefined || refresh === undefined) return;
    await this.platform.disk.set(
      "sealedTokens",
      toBase64(this.core.sealTokens(access, refresh)),
    );
  }

  private async persist(): Promise<void> {
    await this.platform.disk.putSnapshot(this.core.snapshot());
  }

  /** Keeps the unlocked session in memory-only storage and restarts the auto-lock timer. */
  private async afterUnlock(): Promise<void> {
    await this.platform.session.set(this.core.exportSession());
    await this.touch();
  }

  /**
   * Creates the account, then its first vault. The Secret Key goes back to
   * the popup once, for the Emergency Kit; it also stays on this device.
   */
  private async signUp(
    email: string,
    password: string,
  ): Promise<{ secretKey: string }> {
    const deviceId = await this.deviceId();
    const done = await this.run(() =>
      this.core.startSignup(
        email,
        password,
        deviceId,
        "extension",
        DEVICE_NAME,
      ),
    );
    if (done.outcome !== "signed_up" || done.secretKey === undefined)
      throw new ServiceError("internal");
    await this.signedIn(email, done.secretKey);
    return { secretKey: done.secretKey };
  }

  private async signIn(
    email: string,
    password: string,
    secretKey: string,
  ): Promise<void> {
    const deviceId = await this.deviceId();
    const done = await this.run(() =>
      this.core.startLogin(email, password, secretKey, deviceId, "extension"),
    );
    if (done.outcome !== "logged_in") throw new ServiceError("internal");
    await this.signedIn(email, secretKey);
  }

  /** After sign-up or sign-in: remember the account, keep the session, make sure a vault exists. */
  private async signedIn(email: string, secretKey: string): Promise<void> {
    await this.platform.disk.set("email", email);
    await this.platform.disk.set("secretKey", secretKey);
    await this.afterUnlock();
    try {
      await this.handleOutcome(
        await this.run(() => this.core.startEnsureVault()),
      );
    } catch (error) {
      // Signed in all the same; the next sync makes the vault.
      this.syncError = error instanceof ServiceError ? error.code : "internal";
    }
  }

  private async unlock(password: string): Promise<void> {
    const secretKey = await this.platform.disk.get("secretKey");
    if (secretKey === undefined) throw new ServiceError("signed_out");
    try {
      this.core.unlockWithSealedTokens(
        password,
        secretKey,
        await this.sealedTokens(),
      );
    } catch (error) {
      throw new ServiceError(errorCode(error));
    }
    await this.afterUnlock();
    void this.sync();
  }

  async lock(): Promise<void> {
    this.core.lock();
    await this.platform.session.clear();
    await this.platform.alarms.clear("autolock");
  }

  private async signOut(): Promise<void> {
    await this.lock();
    await this.platform.disk.remove(["email", "secretKey", "sealedTokens"]);
    this.core = this.fns.newCore();
    await this.persist();
    await this.loadRules();
  }

  /** Restarts the auto-lock timer (D34: 15 minutes idle by default). */
  private async touch(): Promise<void> {
    if (!this.core.isUnlocked()) return;
    await this.platform.alarms.set(
      "autolock",
      this.platform.now() + this.settings.autoLockMinutes * 60_000,
    );
  }

  private storedItems(): { vaultId: string; item: StoredItem }[] {
    const out: { vaultId: string; item: StoredItem }[] = [];
    for (const vaultId of this.core.vaultIds()) {
      for (const json of this.core.items(vaultId)) {
        out.push({ vaultId, item: JSON.parse(json) as StoredItem });
      }
    }
    return out;
  }

  private items(query: string): ItemSummary[] {
    const needle = query.trim().toLowerCase();
    return this.storedItems()
      .filter(
        ({ item }) => item.trashed_at === undefined || item.trashed_at === null,
      )
      .filter(({ item }) => text(item.type) !== "folder")
      .map(({ vaultId, item }) => ({
        vaultId,
        itemId: text(item.id),
        title: text(item.title),
        username: text(item.username),
        url: firstUrl(item.urls),
        hasPassword: text(item.password) !== "",
        hasCode: text(item.totp) !== "",
      }))
      .filter(
        (s) =>
          needle === "" ||
          [s.title, s.username, s.url].some((v) =>
            v.toLowerCase().includes(needle),
          ),
      )
      .sort((a, b) => a.title.localeCompare(b.title));
  }

  private async copy(
    vaultId: string,
    itemId: string,
    field: "username" | "password" | "code",
  ): Promise<void> {
    let value: string;
    if (field === "code") {
      const code = this.core.currentCode(vaultId, itemId, this.platform.now());
      value = code?.[0] ?? "";
    } else {
      const found = this.storedItems().find(
        (s) => s.vaultId === vaultId && text(s.item.id) === itemId,
      );
      if (!found) throw new ServiceError("unknown_item");
      value = text(found.item[field]);
    }
    await this.writeClipboard(value);
  }

  /** Copies, and clears the clipboard 30 seconds later whatever it holds then (D34). */
  private async writeClipboard(value: string): Promise<void> {
    if (value === "") throw new ServiceError("empty");
    await this.platform.clipboard.write(value);
    await this.platform.alarms.set(
      "clipboard",
      this.platform.now() + CLIPBOARD_CLEAR_MS,
    );
  }

  /** One sync run; concurrent requests share it. */
  sync(): Promise<Status> {
    this.syncing ??= this.syncOnce().finally(() => {
      this.syncing = null;
    });
    return this.syncing;
  }

  private async syncOnce(): Promise<Status> {
    if (!this.core.isUnlocked()) return this.status();
    try {
      await this.handleOutcome(await this.run(() => this.core.startSync()));
    } catch (error) {
      this.syncError = error instanceof ServiceError ? error.code : "internal";
    }
    return this.status();
  }

  private async handleOutcome(step: CoreStep): Promise<void> {
    if (step.kind === "wait") {
      this.syncError = "offline";
      await this.platform.alarms.set(
        "sync-retry",
        this.platform.now() + step.delayMs,
      );
      return;
    }
    switch (step.outcome) {
      case "synced":
      case "vault_created":
        this.syncedAt = this.platform.now();
        this.syncError = null;
        return;
      case "signed_out":
        this.syncError = "signed_out";
        return;
      case "device_revoked":
        // The core cleared the cache; nothing of the account stays here.
        await this.platform.session.clear();
        await this.platform.disk.remove(["email", "secretKey", "sealedTokens"]);
        this.syncError = "device_revoked";
        return;
      default:
        this.syncError = "internal";
    }
  }

  private async setSettings(settings: Settings): Promise<Settings> {
    const url = settings.serverUrl.trim().replace(/\/+$/, "");
    if (
      !/^https:\/\/[^/\s]+$/.test(url) &&
      !/^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/.test(url)
    ) {
      throw new ServiceError("invalid_server");
    }
    if (
      !(AUTO_LOCK_CHOICES as readonly number[]).includes(
        settings.autoLockMinutes,
      )
    ) {
      throw new ServiceError("invalid_auto_lock");
    }
    // The account lives on one server: changing it means signing out first.
    if (
      url !== this.settings.serverUrl &&
      this.core.accountId() !== undefined
    ) {
      throw new ServiceError("signed_in");
    }
    this.settings = {
      serverUrl: url,
      autoLockMinutes: settings.autoLockMinutes,
    };
    await this.platform.disk.set("settings", JSON.stringify(this.settings));
    await this.touch();
    return this.settings;
  }

  /** An alarm fired. */
  async onAlarm(name: AlarmName): Promise<void> {
    switch (name) {
      case "autolock":
        await this.lock();
        return;
      case "clipboard":
        await this.platform.clipboard.write("");
        return;
      case "sync-retry":
        await this.sync();
        return;
      case "rules":
        await this.updateRules();
        return;
    }
  }

  /** The computer locked (`chrome.idle`): lock at once (D34). */
  async onScreenLocked(): Promise<void> {
    await this.lock();
  }

  /**
   * Downloads the signed rules file (D33, D34) and schedules the next try
   * in about 24 hours. Refused or failed downloads keep the current rules.
   */
  async updateRules(): Promise<void> {
    try {
      const etag = await this.platform.disk.get("rulesEtag");
      const response = await this.platform.fetch(HOSTED_SERVER + RULES_PATH, {
        method: "GET",
        headers: etag ? { "if-none-match": etag } : {},
        credentials: "omit",
        cache: "no-store",
        redirect: "error",
      });
      if (response.status === 200) {
        const minVersion = Number(
          (await this.platform.disk.get("rulesVersion")) ?? "0",
        );
        const bytes = new Uint8Array(await response.arrayBuffer());
        const version = this.core.setAutofillRules(
          bytes,
          this.platform.now(),
          minVersion,
        );
        await this.platform.disk.set("rules", toBase64(bytes));
        await this.platform.disk.set("rulesVersion", String(version));
        const newEtag = response.headers.get("etag");
        if (newEtag) await this.platform.disk.set("rulesEtag", newEtag);
      }
    } catch {
      // Offline, refused or not published: keep what we have.
    }
    const spread = (this.platform.random() * 2 - 1) * RULES_SPREAD_MS;
    await this.platform.alarms.set(
      "rules",
      this.platform.now() + RULES_EVERY_MS + spread,
    );
  }
}

function parseSettings(stored: string): Settings {
  try {
    const value = JSON.parse(stored) as Partial<Settings>;
    return {
      serverUrl:
        typeof value.serverUrl === "string" ? value.serverUrl : HOSTED_SERVER,
      autoLockMinutes:
        typeof value.autoLockMinutes === "number" &&
        (AUTO_LOCK_CHOICES as readonly number[]).includes(value.autoLockMinutes)
          ? value.autoLockMinutes
          : DEFAULT_SETTINGS.autoLockMinutes,
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

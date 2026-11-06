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
import { localDate, paperFor } from "./kit.ts";
import { ServiceError } from "./errors.ts";
import { Approvals } from "./approvals.ts";
import { TouchIdUnlock } from "./touchid.ts";
import { Autofill } from "./fill.ts";
import { Importer } from "./importer.ts";
import { Manager } from "./manage.ts";
import { Passkeys } from "./passkeys.ts";
import { Reporter } from "./report.ts";
import { Saver } from "./save.ts";
import type { Core, CoreFunctions, CoreStep } from "./core.ts";
import { runSteps } from "./http.ts";
import type { AlarmName, Platform } from "./platform.ts";

export const RULES_PATH = "/autofill-rules.bin";
const CLIPBOARD_CLEAR_MS = 30_000;
/** How long signing out waits for the server (D62). */
const SIGN_OUT_MS = 5_000;
const RULES_EVERY_MS = 24 * 60 * 60 * 1000;
const RULES_SPREAD_MS = 2 * 60 * 60 * 1000;
const DEVICE_NAME = "Browser extension";
const DEFAULT_SETTINGS: Settings = {
  serverUrl: HOSTED_SERVER,
  autoLockMinutes: 15,
};

export { ServiceError };

/** An item as the core lists it (D18 JSON): login fields are under `login`. */
interface StoredItem {
  id?: unknown;
  type?: unknown;
  title?: unknown;
  trashed_at?: unknown;
  login?: unknown;
}

interface LoginFields {
  username: string;
  password: string;
  totp: string;
  url: string;
}

function loginFields(item: StoredItem): LoginFields {
  const login = (
    typeof item.login === "object" && item.login !== null ? item.login : {}
  ) as Record<string, unknown>;
  return {
    username: text(login.username),
    password: text(login.password),
    totp: text(login.totp),
    url: firstUrl(login.urls),
  };
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
  /** Filling on user action (D35). */
  readonly autofill: Autofill;
  /** Save and update offers (D36). */
  readonly saver: Saver;
  /** Passkeys (D37). */
  readonly passkeys: Passkeys;
  /** Autofill failure reports (D38). */
  readonly reporter: Reporter;
  /** Devices, the health report and export (D44). */
  readonly manager: Manager;
  readonly importer: Importer;
  /** Approving a new device from a signed-in one (D46). */
  readonly approvals: Approvals;
  private readonly touchId: TouchIdUnlock;

  private constructor(
    private readonly fns: CoreFunctions,
    private readonly platform: Platform,
    core: Core,
  ) {
    this.core = core;
    this.autofill = new Autofill({
      core: () => this.core,
      platform,
      copy: (text) => this.writeClipboard(text),
      touch: () => this.touch(),
      login: (vaultId, itemId) => this.login(vaultId, itemId),
    });
    this.saver = new Saver({
      core: () => this.core,
      platform,
      autofill: this.autofill,
      enabled: () => this.autofill.inlineIcon(),
      saved: async () => {
        await this.persist();
        void this.sync();
      },
      touch: () => this.touch(),
    });
    this.autofill.onTopFrame = (sender) => this.saver.onTopFrame(sender);
    this.reporter = new Reporter({
      core: () => this.core,
      platform,
      autofill: this.autofill,
    });
    this.manager = new Manager({
      core: () => this.core,
      platform,
      fns,
      run: (start) => this.run(start),
      ended: (step) => this.handleOutcome(step),
      secretKey: () => platform.disk.get("secretKey"),
      touch: () => this.touch(),
    });
    this.importer = new Importer({
      core: () => this.core,
      fns,
      platform,
      touch: () => this.touch(),
      saved: async () => {
        await this.persist();
        void this.sync();
      },
    });
    this.approvals = new Approvals({
      core: () => this.core,
      fns,
      platform,
      server: () => this.settings.serverUrl,
      run: (start) => this.run(start),
      ended: (step) => this.handleOutcome(step),
      secretKey: () => platform.disk.get("secretKey"),
      signIn: (email, password, secretKey) =>
        this.signIn(email, password, secretKey),
    });
    this.touchId = new TouchIdUnlock({
      core: () => this.core,
      platform,
      deviceId: () => this.deviceId(),
      sealedTokens: () => this.sealedTokens(),
    });
    this.passkeys = new Passkeys({
      core: () => this.core,
      platform,
      enabled: async () =>
        (await platform.disk.get("passkeys")) === "on" &&
        (await platform.pages.hasSiteAccess()),
      saved: async () => {
        await this.persist();
        void this.sync();
      },
      unlock: async (password) => {
        await this.handle({ kind: "unlock", password });
      },
      touch: () => this.touch(),
    });
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
      case "signInAgain":
        await this.signInAgain(request.password);
        return this.status();
      case "unlockTouchId":
        await this.touchId.unlock();
        await this.afterUnlock();
        void this.sync();
        return null;
      case "touchId":
        return this.touchId.isOn();
      case "setTouchId":
        await this.touch();
        if (request.on) await this.touchId.turnOn();
        else await this.touchId.turnOff();
        return this.touchId.isOn();
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
        // Copying any field by its place belongs to the item screens (D67).
        if (!("field" in request)) throw new ServiceError("unsupported");
        await this.touch();
        await this.copy(request.vaultId, request.itemId, request.field);
        return null;
      case "copySecretKey": {
        const secretKey = await this.platform.disk.get("secretKey");
        if (secretKey === undefined) throw new ServiceError("signed_out");
        await this.writeClipboard(secretKey);
        return null;
      }
      case "emergencyKit":
        return { pdf: toBase64(await this.emergencyKit()) };
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
      case "suggestions":
        await this.touch();
        return this.autofill.suggestions();
      case "fill":
        return this.autofill.fillActiveTab(
          request.vaultId,
          request.itemId,
          request.confirmed,
        );
      case "inlineIcon":
        return this.autofill.inlineIcon();
      case "setInlineIcon":
        return this.autofill.setInlineIcon(request.on);
      case "saveOffer":
        return this.saver.popupOffer();
      case "acceptOffer": {
        const edits: { title?: string; username?: string } = {};
        if (request.title !== undefined) edits.title = request.title;
        if (request.username !== undefined) edits.username = request.username;
        return this.saver.acceptFromPopup(request.itemId, edits);
      }
      case "dismissOffer":
        return this.saver.dismissFromPopup(request.never);
      case "captureNow":
        return this.saver.captureActiveTab();
      case "passkeys":
        return (
          (await this.platform.disk.get("passkeys")) === "on" &&
          (await this.platform.pages.hasSiteAccess())
        );
      case "setPasskeys":
        return this.passkeys.setEnabled(request.on);
      case "reportPreview":
        return this.reporter.preview(request.problem);
      case "reportSend":
        return this.reporter.send();
      case "activity":
        await this.touch();
        return null;
      case "devices":
        return this.manager.devices();
      case "renameDevice":
        return this.manager.rename(request.deviceId, request.name);
      case "revokeDevice":
        return this.manager.revoke(request.deviceId);
      case "health":
        return this.manager.health();
      case "checkLeaks":
        return this.manager.checkLeaks();
      case "setLeakCheck":
        return this.manager.setLeakCheck(request.on);
      case "askApproval":
        return this.approvals.ask(request.email);
      case "askApprovalPoll":
        return this.approvals.poll();
      case "askApprovalCancel":
        this.approvals.cancel();
        return null;
      case "signInApproved":
        await this.approvals.signInApproved(request.password);
        return null;
      case "approvals":
        await this.touch();
        return this.approvals.list();
      case "acceptApproval":
        await this.touch();
        return this.approvals.accept(request.id);
      case "completeApproval":
        await this.touch();
        return this.approvals.complete(request.id);
      case "denyApproval":
        await this.touch();
        return this.approvals.deny(request.id);
      case "exportVault":
        return this.manager.exportVault(
          request.masterPassword,
          request.format,
          request.exportPassword,
        );
      case "importDetect":
        return this.importer.detect(request.name, request.file);
      case "importPrepare":
        return this.importer.prepare(
          request.file,
          request.format,
          request.password,
          request.keyFile,
          request.columns,
        );
      case "importFinish":
        return this.importer.finish();
      case "importCancel":
        this.importer.cancel();
        return null;
      // The desktop's item screens (D67); the extension gets them in M4.14.
      case "vault":
      case "item":
      case "reveal":
      case "itemForEdit":
      case "saveItem":
      case "trashItem":
      case "restoreItem":
      case "deleteItem":
      case "newFolder":
      case "totpSetup":
        throw new ServiceError("unsupported");
      // The popup doesn't offer Touch ID after unlocking (D48 §5).
      case "biometricOffer":
        return false;
      case "dismissBiometricOffer":
        return null;
    }
  }

  async status(): Promise<Status> {
    if (this.core.accountId() === undefined) return { state: "signedOut" };
    const email = (await this.platform.disk.get("email")) ?? "";
    if (!this.core.isUnlocked())
      return { state: "locked", email, touchId: await this.touchId.isOn() };
    return {
      state: "unlocked",
      email,
      syncedAt: this.syncedAt,
      syncError: this.syncError,
      approvals: this.approvals.pending,
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
      this.core.startLogin(
        email,
        password,
        secretKey,
        deviceId,
        "extension",
        DEVICE_NAME,
      ),
    );
    if (done.outcome !== "logged_in") throw new ServiceError("internal");
    await this.signedIn(email, secretKey);
  }

  /**
   * After the server refused this browser's refresh token (D50 §5): the
   * master password only, since the email and Secret Key are here. The cache
   * and its unsent changes stay, and so does Touch ID; it counts as typing
   * the master password. Then syncs.
   */
  private async signInAgain(password: string): Promise<void> {
    const email = await this.platform.disk.get("email");
    const secretKey = await this.platform.disk.get("secretKey");
    if (email === undefined || secretKey === undefined)
      throw new ServiceError("signed_out");
    const deviceId = await this.deviceId();
    // An empty name sends none: the device keeps its name.
    const done = await this.run(() =>
      this.core.startLogin(
        email,
        password,
        secretKey,
        deviceId,
        "extension",
        "",
      ),
    );
    if (done.outcome !== "logged_in") throw new ServiceError("internal");
    await this.touchId.passwordTyped();
    await this.afterUnlock();
    this.syncError = null;
    await this.sync();
  }

  /** The Emergency Kit for the signed-in account, on this browser's paper size. */
  private async emergencyKit(): Promise<Uint8Array> {
    const email = await this.platform.disk.get("email");
    const secretKey = await this.platform.disk.get("secretKey");
    const accountId = this.core.accountId();
    if (
      email === undefined ||
      secretKey === undefined ||
      accountId === undefined
    )
      throw new ServiceError("signed_out");
    const server =
      this.settings.serverUrl === HOSTED_SERVER
        ? undefined
        : this.settings.serverUrl;
    return this.fns.emergencyKit(
      email,
      secretKey,
      server,
      accountId,
      localDate(this.platform.now()),
      paperFor(this.platform.locale),
    );
  }

  /** After sign-up or sign-in: remember the account, keep the session, make sure a vault exists. */
  private async signedIn(email: string, secretKey: string): Promise<void> {
    await this.platform.disk.set("email", email);
    await this.platform.disk.set("secretKey", secretKey);
    // A new sign-in starts with Touch ID off (as D45).
    await this.platform.disk.remove(["browserUnlock"]);
    await this.touchId.passwordTyped();
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
    await this.touchId.passwordTyped();
    await this.afterUnlock();
    void this.sync();
  }

  async lock(): Promise<void> {
    this.core.lock();
    this.manager.clear();
    await this.saver.clear();
    await this.platform.session.clear();
    await this.platform.alarms.clear("autolock");
  }

  private async signOut(): Promise<void> {
    await this.endSession();
    await this.touchId.turnOff();
    await this.lock();
    await this.platform.disk.remove([
      "email",
      "secretKey",
      "sealedTokens",
      "passwordUnlockedAt",
    ]);
    this.core = this.fns.newCore();
    await this.persist();
    await this.loadRules();
  }

  /**
   * Ends this device's session on the server (D62): at most
   * `SIGN_OUT_MS`, any failure ignored, so being offline never blocks
   * signing out. Unlocked only: the tokens are sealed while locked (D34),
   * and Settings, where sign-out is, needs the vault unlocked.
   */
  private async endSession(): Promise<void> {
    if (!this.core.isUnlocked()) return;
    const ended = this.run(() => this.core.startSignOut()).then(
      () => undefined,
      () => undefined,
    );
    let timer: ReturnType<typeof setTimeout> | undefined;
    const late = new Promise<void>((resolve) => {
      timer = setTimeout(resolve, SIGN_OUT_MS);
    });
    await Promise.race([ended, late]);
    clearTimeout(timer);
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
        username: loginFields(item).username,
        url: loginFields(item).url,
        hasPassword: loginFields(item).password !== "",
        hasCode: loginFields(item).totp !== "",
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

  /**
   * Saves an item (D18 JSON with an `id`) into the first vault. Used by the
   * browser tests now and by the save prompts (M4.4).
   */
  async saveItem(json: string): Promise<{ vaultId: string; itemId: string }> {
    const vaultId = this.core.vaultIds()[0];
    if (vaultId === undefined) throw new ServiceError("no_vault");
    const itemId = this.core.saveItem(vaultId, json, this.platform.now());
    await this.persist();
    return { vaultId, itemId };
  }

  /** A login's fill values, for the autofill only (never sent to the popup). */
  private login(
    vaultId: string,
    itemId: string,
  ): { username: string; password: string; hasCode: boolean } | undefined {
    const found = this.storedItems().find(
      (s) => s.vaultId === vaultId && text(s.item.id) === itemId,
    );
    if (!found || text(found.item.type) !== "login") return undefined;
    return {
      username: loginFields(found.item).username,
      password: loginFields(found.item).password,
      hasCode: loginFields(found.item).totp !== "",
    };
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
      value = loginFields(found.item)[field];
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
    if (this.core.isUnlocked() && this.syncError === null) {
      await this.approvals.refresh();
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

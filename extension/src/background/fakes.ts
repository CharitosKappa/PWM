/**
 * Test doubles: a scripted core and an in-memory platform. The real core is
 * tested in Rust and, through WASM, in `wasm.test.ts`.
 */
import type { Core, CoreAskStep, CoreFunctions, CoreStep } from "./core.ts";
import type {
  ContentMessage,
  FieldInfo,
  FillReply,
  Typed,
} from "../shared/autofill.ts";
import type {
  AlarmName,
  Disk,
  DiskKey,
  FrameRef,
  FrameSender,
  NativeHost,
  Pages,
  Platform,
} from "./platform.ts";

export const PASSWORD = "correct horse battery staple";
export const SECRET_KEY = "A3-ABCDEF-GHJKLM-NPQRST-UVWXYZ-23456-789AB";
export const ITEM_PASSWORD = "hunter2-item-password";
export const TOTP_SECRET = "JBSWY3DPEHPK3PXP";
export const EXPORT_PASSWORD = "granite sparrow lattice 91";

function coreError(code: string): Error {
  return Object.assign(new Error(code), { code });
}

export function send(path: string): CoreStep {
  return {
    kind: "send",
    method: "POST",
    path,
    headers: ["content-type", "application/json"],
    delayMs: 0,
    report: [],
  };
}

export function done(outcome: string, extra: Partial<CoreStep> = {}): CoreStep {
  return {
    kind: "done",
    headers: [],
    delayMs: 0,
    report: [],
    outcome,
    ...extra,
  };
}

export function wait(delayMs: number): CoreStep {
  return { kind: "wait", headers: [], delayMs, report: [] };
}

/** A core whose server conversations are one request each, answered by `next`. */
export class FakeCore implements Core {
  unlocked = false;
  account: string | undefined;
  items_ = new Map<string, string[]>();
  tokens: string[] | undefined;
  /** What the one pending request turns into. */
  next: () => CoreStep = () => done("synced");
  /** What the next sync ends with. */
  syncResult: () => CoreStep = () => done("synced");
  rulesVersions: { version: number; minVersion: number }[] = [];
  restoreFails = false;
  cancelled = 0;
  /** The device name each login sent. */
  loginNames: string[] = [];

  snapshot(): Uint8Array {
    return new TextEncoder().encode(
      JSON.stringify({ account: this.account, items: [...this.items_] }),
    );
  }

  accountId(): string | undefined {
    return this.account;
  }

  isUnlocked(): boolean {
    return this.unlocked;
  }

  startSignup(
    email: string,
    password: string,
    deviceId: string,
    platform: string,
  ): CoreStep {
    if (deviceId === "" || platform !== "extension")
      throw coreError("invalid_id");
    this.next = () => {
      if (email === "taken@example.com") throw coreError("account_exists");
      if (password !== PASSWORD) throw coreError("weak_password");
      this.account = "acct-1";
      this.unlocked = true;
      this.tokens = ["access-1", "refresh-1"];
      return done("signed_up", { secretKey: SECRET_KEY });
    };
    return send("/v1/auth/signup");
  }

  startLogin(
    email: string,
    password: string,
    secretKey: string,
    deviceId: string,
    platform: string,
    deviceName: string,
  ): CoreStep {
    if (deviceId === "" || platform !== "extension")
      throw coreError("invalid_id");
    // Empty sends no name, as the core does (signing in again, D50 §5).
    this.loginNames.push(deviceName);
    this.next = () => {
      if (password !== PASSWORD || secretKey !== SECRET_KEY || email === "")
        throw coreError("invalid_credentials");
      this.account = "acct-1";
      this.unlocked = true;
      this.tokens = ["access-1", "refresh-1"];
      return done("logged_in");
    };
    return send("/v1/auth/login");
  }

  startSync(): CoreStep {
    if (!this.unlocked) throw coreError("locked");
    this.next = this.syncResult;
    return send("/v1/sync");
  }

  /** Sessions ended on the server (D62). */
  loggedOut = 0;

  startSignOut(): CoreStep {
    if (!this.unlocked) throw coreError("locked");
    this.next = () => {
      this.loggedOut += 1;
      return done("logged_out");
    };
    return send("/v1/auth/logout");
  }

  startEnsureVault(): CoreStep {
    if (!this.unlocked) throw coreError("locked");
    this.next = () => {
      if (this.items_.size > 0) return done("synced");
      this.items_.set("vault-1", []);
      return done("vault_created", { vaultId: "vault-1" });
    };
    return send("/v1/vaults");
  }

  receive(status: number): CoreStep {
    if (status >= 500) throw coreError("server");
    return this.next();
  }

  networkFailed(): CoreStep {
    throw coreError("network");
  }

  cancel(): void {
    this.cancelled += 1;
  }

  takeTokens(): string[] | undefined {
    const tokens = this.tokens;
    this.tokens = undefined;
    return tokens;
  }

  sealTokens(access: string, refresh: string): Uint8Array {
    if (!this.unlocked) throw coreError("locked");
    // Stands in for real sealing: tests check the raw tokens never reach disk.
    return new TextEncoder().encode(
      `sealed:${String(access.length)}:${String(refresh.length)}`,
    );
  }

  unlockWithSealedTokens(
    password: string,
    secretKey: string,
    sealedTokens: Uint8Array | undefined,
  ): void {
    if (this.account === undefined) throw coreError("signed_out");
    if (password !== PASSWORD || secretKey !== SECRET_KEY)
      throw coreError("wrong_password");
    if (sealedTokens === undefined) throw coreError("signed_out");
    this.unlocked = true;
  }

  exportSession(): Uint8Array {
    if (!this.unlocked) throw coreError("locked");
    return new Uint8Array([1, 2, 3, 4]);
  }

  /** Stands in for the browser blob: the key is 32 bytes of 7, the blob says whose. */
  exportBrowserUnlock(deviceId: string): Uint8Array {
    if (!this.unlocked) throw coreError("locked");
    const blob = new TextEncoder().encode(`blob:${deviceId}`);
    const out = new Uint8Array(32 + blob.length);
    out.fill(7, 0, 32);
    out.set(blob, 32);
    return out;
  }

  unlockBrowser(
    blob: Uint8Array,
    key: Uint8Array,
    deviceId: string,
    sealedTokens: Uint8Array | undefined,
  ): void {
    const ok =
      new TextDecoder().decode(blob) === `blob:${deviceId}` &&
      key.length === 32 &&
      key.every((b) => b === 7);
    if (!ok || this.account === undefined)
      throw coreError("browser_unlock_failed");
    if (sealedTokens === undefined) throw coreError("signed_out");
    this.unlocked = true;
  }

  restoreSession(session: Uint8Array): void {
    if (this.restoreFails || session.length !== 4)
      throw coreError("wrong_password");
    this.unlocked = true;
  }

  lock(): void {
    this.unlocked = false;
  }

  vaultIds(): string[] {
    if (!this.unlocked) throw coreError("locked");
    return [...this.items_.keys()];
  }

  items(vaultId: string): string[] {
    if (!this.unlocked) throw coreError("locked");
    return this.items_.get(vaultId) ?? [];
  }

  currentCode(vaultId: string, itemId: string): string[] | undefined {
    if (!this.unlocked) throw coreError("locked");
    const item = (this.items_.get(vaultId) ?? []).map(
      (j) => JSON.parse(j) as { id: string; login?: { totp?: string } },
    );
    return item.find((i) => i.id === itemId)?.login?.totp
      ? ["287082", "1"]
      : undefined;
  }

  setAutofillRules(
    bytes: Uint8Array,
    _nowMs: number,
    minVersion: number,
  ): number {
    const version = bytes[0] ?? 0;
    if (version === 0xff) throw coreError("invalid_rules");
    if (version < minVersion) throw coreError("invalid_rules");
    this.rulesVersions.push({ version, minVersion });
    return version;
  }

  /** Multi-step state: tab → [itemId, step done]. */
  steps = new Map<string, [string, string]>();

  /**
   * Matches by host name, with the engine's decisions in miniature: `http`
   * warns, a frame on another host is refused.
   */
  autofillMatches(pageUrl: string, frameUrl: string | undefined): string {
    if (!this.unlocked) throw coreError("locked");
    const page = new URL(pageUrl);
    const frameHost =
      frameUrl === undefined ? undefined : new URL(frameUrl).hostname;
    const out: Record<string, string>[] = [];
    for (const [vaultId, list] of this.items_) {
      for (const json of list) {
        const item = JSON.parse(json) as {
          id: string;
          type?: string;
          title?: string;
          trashed_at?: number | null;
          login?: { username?: string; urls?: { url: string }[] };
        };
        if (item.type !== "login" || item.trashed_at != null) continue;
        const first = item.login?.urls?.[0];
        const host = first ? new URL(first.url).hostname : "";
        if (host !== page.hostname) continue;
        let decision = page.protocol === "https:" ? "fill" : "warn_insecure";
        if (frameHost !== undefined && frameHost !== host)
          decision = "refuse_cross_site";
        out.push({
          vault_id: vaultId,
          item_id: item.id,
          title: item.title ?? "",
          username: item.login?.username ?? "",
          matched: "host",
          decision,
        });
      }
    }
    return JSON.stringify(out);
  }

  /** Classifies by type and name only; the real classifier is tested in Rust. */
  classifyFields(fieldsJson: string): string {
    const fields = JSON.parse(fieldsJson) as FieldInfo[];
    return JSON.stringify(
      fields.map((f) => {
        if (f.type === "password")
          return {
            class:
              f.autocomplete === "new-password"
                ? "new-password"
                : "current-password",
            confidence: 0.9,
          };
        if (f.autocomplete === "one-time-code")
          return { class: "one-time-code", confidence: 0.95 };
        if (f.name.includes("otp"))
          return { class: "one-time-code", confidence: 0.5 };
        if (f.type === "email" || f.name.includes("user"))
          return { class: "username", confidence: 0.9 };
        return { class: "ignore", confidence: 0.5 };
      }),
    );
  }

  autofillRecordStep(
    tab: string,
    _pageUrl: string,
    itemId: string,
    step: string,
  ): void {
    this.steps.set(tab, [itemId, step]);
  }

  autofillNextStep(tab: string): string[] | undefined {
    const done = this.steps.get(tab);
    if (!done) return undefined;
    return [done[0], done[1] === "username" ? "password" : "totp"];
  }

  /** The core's decision in miniature (tested for real in Rust). */
  saveOffer(
    pageUrl: string,
    _frameUrl: string | undefined,
    username: string,
    password: string,
    newPassword: string | undefined,
  ): string {
    if (!this.unlocked) throw coreError("locked");
    const host = new URL(pageUrl).hostname;
    const secret = newPassword ?? password;
    const same = this.logins().filter(
      (l) =>
        l.host === host &&
        l.username.toLowerCase() === username.trim().toLowerCase(),
    );
    if (secret === "" || same.some((l) => l.password === secret))
      return JSON.stringify({ kind: "nothing", candidates: [] });
    if (same.length > 0)
      return JSON.stringify({
        kind: "update",
        candidates: same.map((l) => ({
          vault_id: l.vaultId,
          item_id: l.id,
          title: l.title,
          username: l.username,
        })),
      });
    return JSON.stringify({ kind: "save", candidates: [] });
  }

  saveLogin(
    vaultId: string,
    pageUrl: string,
    title: string,
    username: string,
    password: string,
  ): string {
    if (!this.unlocked) throw coreError("locked");
    const id = `saved-${String(this.logins().length + 1)}`;
    this.addLogin(vaultId, {
      id,
      title: title || new URL(pageUrl).hostname,
      username,
      password,
      urls: [{ url: new URL(pageUrl).origin }],
    });
    return id;
  }

  /** Old passwords of updated logins, newest last. */
  history = new Map<string, string[]>();

  updatePassword(vaultId: string, itemId: string, password: string): void {
    if (!this.unlocked) throw coreError("locked");
    const login = this.logins().find((l) => l.id === itemId);
    if (!login) throw coreError("unknown_item");
    this.history.set(itemId, [
      ...(this.history.get(itemId) ?? []),
      login.password,
    ]);
    this.addLogin(vaultId, {
      id: itemId,
      title: login.title,
      username: login.username,
      password,
      urls: [{ url: login.url }],
    });
  }

  private logins() {
    const out: {
      vaultId: string;
      id: string;
      title: string;
      username: string;
      password: string;
      url: string;
      host: string;
    }[] = [];
    for (const [vaultId, list] of this.items_) {
      for (const json of list) {
        const item = JSON.parse(json) as {
          id: string;
          type: string;
          title: string;
          login?: {
            username?: string;
            password?: string;
            urls?: { url: string }[];
          };
        };
        if (item.type !== "login") continue;
        const url = item.login?.urls?.[0]?.url ?? "";
        out.push({
          vaultId,
          id: item.id,
          title: item.title,
          username: item.login?.username ?? "",
          password: item.login?.password ?? "",
          url,
          host: url === "" ? "" : new URL(url).hostname,
        });
      }
    }
    return out;
  }

  /** Passkeys in miniature: one per (origin host, user), id `pk-<n>`. */
  passkeysMade: {
    vaultId: string;
    host: string;
    user: string;
    id: string;
  }[] = [];

  private passkeyRequest(json: string): {
    rp?: { id?: string };
    rpId?: string;
    user?: { name?: string };
    pubKeyCredParams?: { alg: number }[];
    excludeCredentials?: { id: string }[];
    allowCredentials?: { id: string }[];
  } {
    try {
      return JSON.parse(json) as ReturnType<FakeCore["passkeyRequest"]>;
    } catch {
      throw coreError("invalid_passkey_request");
    }
  }

  private passkeyRp(origin: string, rpId: string | undefined): string {
    const host = new URL(origin).hostname;
    if (!origin.startsWith("https:")) throw coreError("insecure_origin");
    if (rpId !== undefined && host !== rpId && !host.endsWith(`.${rpId}`))
      throw coreError("invalid_rp_id");
    return rpId ?? host;
  }

  passkeyCreateCheck(
    origin: string,
    _top: string | undefined,
    json: string,
  ): string {
    const r = this.passkeyRequest(json);
    const rp = this.passkeyRp(origin, r.rp?.id);
    if (r.pubKeyCredParams && !r.pubKeyCredParams.some((p) => p.alg === -7))
      return JSON.stringify({ kind: "unsupported" });
    const excluded = (r.excludeCredentials ?? []).some((c) =>
      this.passkeysMade.some((p) => p.id === c.id),
    );
    if (excluded) return JSON.stringify({ kind: "excluded" });
    return JSON.stringify({
      kind: "ready",
      rp_id: rp,
      user_name: r.user?.name ?? "",
    });
  }

  passkeyCreate(
    vaultId: string,
    origin: string,
    _top: string | undefined,
    json: string,
  ): string {
    if (!this.unlocked) throw coreError("locked");
    const r = this.passkeyRequest(json);
    const host = this.passkeyRp(origin, r.rp?.id);
    const id = `pk-${String(this.passkeysMade.length + 1)}`;
    this.passkeysMade.push({ vaultId, host, user: r.user?.name ?? "", id });
    return JSON.stringify({ id, rawId: id, type: "public-key", response: {} });
  }

  passkeyCandidates(
    origin: string,
    _top: string | undefined,
    json: string,
  ): string {
    if (!this.unlocked) throw coreError("locked");
    const r = this.passkeyRequest(json);
    const host = this.passkeyRp(origin, r.rpId);
    const allow = (r.allowCredentials ?? []).map((c) => c.id);
    return JSON.stringify(
      this.passkeysMade
        .filter(
          (p) =>
            p.host === host && (allow.length === 0 || allow.includes(p.id)),
        )
        .map((p) => ({
          vault_id: p.vaultId,
          item_id: `item-${p.id}`,
          credential_id: p.id,
          user_name: p.user,
          title: p.host,
        })),
    );
  }

  passkeyGet(
    _origin: string,
    _top: string | undefined,
    _json: string,
    _vaultId: string,
    _itemId: string,
    credentialId: string,
  ): string {
    if (!this.unlocked) throw coreError("locked");
    if (!this.passkeysMade.some((p) => p.id === credentialId))
      throw coreError("unknown_item");
    return JSON.stringify({
      id: credentialId,
      type: "public-key",
      response: { signature: "sig" },
    });
  }

  /** Calls to autofillReport, for the tests to look at. */
  reports: { pageUrl: string; problem: string; framesJson: string }[] = [];

  autofillReport(
    pageUrl: string,
    problem: string,
    framesJson: string,
    extensionVersion: string,
    browser: string,
  ): string {
    this.reports.push({ pageUrl, problem, framesJson });
    const frames = JSON.parse(framesJson) as {
      relation: string;
      fields: { form: number | null; type: string }[];
    }[];
    return JSON.stringify({
      v: 1,
      site: new URL(pageUrl).origin,
      problem,
      matched: this.unlocked ? true : null,
      software: {
        extension: extensionVersion,
        core: "0.0.0",
        rules: 0,
        browser,
      },
      frames: frames.map((f) => ({
        relation: f.relation,
        fields: f.fields.map((x) => ({
          form: x.form,
          class: x.type === "password" ? "current-password" : "username",
        })),
      })),
    });
  }

  saveItem(vaultId: string, json: string): string {
    if (!this.unlocked) throw coreError("locked");
    const item = JSON.parse(json) as { id: string };
    const list = (this.items_.get(vaultId) ?? []).filter(
      (j) => (JSON.parse(j) as { id: string }).id !== item.id,
    );
    list.push(json);
    this.items_.set(vaultId, list);
    return item.id;
  }

  autofillForgetTab(tab: string): void {
    this.steps.delete(tab);
  }

  // Devices, health and export (D43, D44).

  devices_: FakeDevice[] = [
    {
      id: "device-1",
      platform: "extension",
      name: "Browser extension",
      created_at: 1_000,
      last_seen_at: 2_000,
      revoked_at: null,
      current: true,
    },
    {
      id: "device-2",
      platform: "ios",
      name: "iPhone",
      created_at: 1_500,
      last_seen_at: 1_800,
      revoked_at: null,
      current: false,
    },
  ];
  health_: { weak: { item_id: string; score: number }[]; reused: string[][] } =
    { weak: [], reused: [] };
  breachPrefixes = ["ABCDE", "12345"];
  breached: { item_id: string; count: number }[] = [];
  breachBodies: [string, string][] = [];
  exportGrant = false;
  authorizations = 0;

  startListDevices(): CoreStep {
    if (!this.unlocked) throw coreError("locked");
    this.next = () =>
      done("devices", { devices: JSON.stringify(this.devices_) });
    return send("/v1/devices");
  }

  startRenameDevice(deviceId: string, name: string): CoreStep {
    if (!this.unlocked) throw coreError("locked");
    if (name.length > 63) throw coreError("invalid_device_name");
    this.next = () => {
      const device = this.devices_.find((d) => d.id === deviceId);
      if (!device || device.revoked_at !== null)
        throw coreError("unknown_device");
      device.name = name;
      return done("device_updated");
    };
    return send(`/v1/devices/${deviceId}`);
  }

  startRevokeDevice(deviceId: string): CoreStep {
    if (!this.unlocked) throw coreError("locked");
    this.next = () => {
      const device = this.devices_.find((d) => d.id === deviceId);
      if (!device) throw coreError("unknown_device");
      device.revoked_at = 3_000;
      return done("device_updated");
    };
    return send(`/v1/devices/${deviceId}`);
  }

  healthReport(): string {
    if (!this.unlocked) throw coreError("locked");
    return JSON.stringify(this.health_);
  }

  breachCheck(): FakeBreachCheck {
    if (!this.unlocked) throw coreError("locked");
    return new FakeBreachCheck(this);
  }

  authorizeExport(password: string, secretKey: string): void {
    if (!this.unlocked) throw coreError("locked");
    this.authorizations += 1;
    if (password !== PASSWORD || secretKey !== SECRET_KEY)
      throw coreError("wrong_password");
    this.exportGrant = true;
  }

  // Device approvals (D46), the signed-in side.

  approvals_: {
    id: string;
    platform: string;
    created_at: number;
    state: string;
    code: string | null;
  }[] = [];
  completedWith: string | null = null;

  startListApprovals(): CoreStep {
    if (!this.unlocked) throw coreError("locked");
    this.next = () =>
      done("approvals", { approvals: JSON.stringify(this.approvals_) });
    return send("/v1/approvals");
  }

  startAcceptApproval(approvalId: string): CoreStep {
    if (!this.unlocked) throw coreError("locked");
    this.next = () => {
      const approval = this.approvals_.find((a) => a.id === approvalId);
      if (!approval) throw coreError("unknown_approval");
      approval.state = "revealed";
      approval.code = "654 321";
      return done("approval_updated");
    };
    return send(`/v1/approvals/${approvalId}/accept`);
  }

  startCompleteApproval(approvalId: string, secretKey: string): CoreStep {
    if (!this.unlocked) throw coreError("locked");
    const approval = this.approvals_.find((a) => a.id === approvalId);
    if (approval?.code === null || approval === undefined)
      throw coreError("unknown_approval");
    this.next = () => {
      this.completedWith = secretKey;
      this.approvals_ = this.approvals_.filter((a) => a.id !== approvalId);
      return done("approval_updated");
    };
    return send(`/v1/approvals/${approvalId}/complete`);
  }

  startDenyApproval(approvalId: string): CoreStep {
    if (!this.unlocked) throw coreError("locked");
    this.next = () => {
      this.approvals_ = this.approvals_.filter((a) => a.id !== approvalId);
      return done("approval_updated");
    };
    return send(`/v1/approvals/${approvalId}/deny`);
  }

  exportFile(format: string, exportPassword: string): Uint8Array {
    if (!this.unlocked) throw coreError("locked");
    if (!this.exportGrant) throw coreError("export_not_authorized");
    this.exportGrant = false;
    return new TextEncoder().encode(
      `export:${format}${format === "pwmx" ? `:${exportPassword}` : ""}`,
    );
  }

  // Import (D51): a CSV's lines after the header become logins named by
  // their first cell; `bitwarden` needs the password "export-pw".
  pendingImport: string[] | null = null;
  importsPrepared = 0;

  prepareImport(
    vaultId: string,
    format: string,
    bytes: Uint8Array,
    password: string,
    keyFile: Uint8Array,
  ): string {
    if (format === "keepass" && password === "" && keyFile.length === 0)
      throw coreError("import_password_needed");
    if (!this.unlocked) throw coreError("locked");
    if (vaultId !== "") throw coreError("unknown_vault");
    if (format === "bitwarden" && password === "")
      throw coreError("import_password_needed");
    if (format === "bitwarden" && password !== "export-pw")
      throw coreError("import_wrong_password");
    if (!["lastpass", "csv", "bitwarden", "keepass"].includes(format))
      throw coreError("import_unreadable");
    const lines = new TextDecoder()
      .decode(bytes)
      .split(/\r?\n/)
      .slice(1)
      .filter((l) => l.trim() !== "");
    this.pendingImport = lines.map((l) => l.split(",")[0] ?? "");
    this.importsPrepared += 1;
    return JSON.stringify({
      logins: lines.length,
      notes: 0,
      cards: 0,
      identities: 0,
      folders: 0,
      trashed: 0,
      left_out: [["attachments", 1]],
    });
  }

  finishImport(): string {
    if (this.pendingImport === null) throw coreError("not_running");
    const titles = this.pendingImport;
    this.pendingImport = null;
    for (const title of titles) {
      this.addLogin("vault-1", { id: `imported-${title}`, title });
    }
    return JSON.stringify({
      logins: titles.length,
      notes: 0,
      cards: 0,
      identities: 0,
      folders: 0,
      trashed: 0,
      left_out: [["attachments", 1]],
    });
  }

  cancelImport(): void {
    this.pendingImport = null;
  }

  /**
   * Adds an item in the core's JSON shape (D18): login fields go under
   * `login`, as `items()` lists them. Same id replaces.
   */
  addLogin(vaultId: string, item: Record<string, unknown>): void {
    const { id, type, title, trashed_at, username, password, totp, urls } =
      item;
    const json = {
      id,
      v: 1,
      type: type ?? "login",
      title: title ?? "",
      notes: "",
      created_at: 0,
      updated_at: 0,
      trashed_at: trashed_at ?? null,
      ...((type ?? "login") === "login"
        ? { login: { username, password, totp, urls: urls ?? [] } }
        : {}),
    };
    const list = (this.items_.get(vaultId) ?? []).filter(
      (j) => (JSON.parse(j) as { id: unknown }).id !== id,
    );
    list.push(JSON.stringify(json));
    this.items_.set(vaultId, list);
  }
}

/**
 * A new device's request (D46) whose relay is scripted: each `receive`
 * gives the next of `steps`. Requests go to `/v1/approvals...`.
 */
export class FakeApprovalAsk {
  steps: { kind: string; code?: string; secretKey?: string }[];
  cancelled = false;

  constructor(steps: { kind: string; code?: string; secretKey?: string }[]) {
    this.steps = steps;
  }

  firstRequest(): CoreStep {
    return send("/v1/approvals");
  }

  poll(): CoreStep {
    return { ...send("/v1/approvals/ask-1"), method: "GET" };
  }

  cancel(): CoreStep {
    this.cancelled = true;
    return { ...send("/v1/approvals/ask-1"), method: "DELETE" };
  }

  receive(): CoreAskStep {
    const next = this.steps.shift() ?? { kind: "wait" };
    return {
      kind: next.kind,
      delayMs: 2000,
      ...(next.code === undefined ? {} : { code: next.code }),
      ...(next.secretKey === undefined ? {} : { secretKey: next.secretKey }),
      request: () =>
        next.kind === "send" ? send("/v1/approvals/ask-1/reveal") : undefined,
    };
  }
}

export interface FakeDevice {
  id: string;
  platform: string;
  name: string | null;
  created_at: number;
  last_seen_at: number;
  revoked_at: number | null;
  current: boolean;
}

/** Records each range body; finishes with the core's `breached` list. */
export class FakeBreachCheck {
  constructor(private readonly core: FakeCore) {}

  prefixes(): string[] {
    return this.core.breachPrefixes;
  }

  addResponse(prefix: string, body: string): void {
    if (!this.core.breachPrefixes.includes(prefix)) throw coreError("protocol");
    if (body === "not a range") throw coreError("protocol");
    this.core.breachBodies.push([prefix, body]);
  }

  finish(): string {
    return JSON.stringify(this.core.breached);
  }
}

/** Core functions that hand out `FakeCore`s; `cores` keeps every one made. */
export function fakeFunctions(): CoreFunctions & {
  cores: FakeCore[];
  asks: FakeApprovalAsk[];
  nextAsk: { kind: string; code?: string; secretKey?: string }[];
} {
  const cores: FakeCore[] = [];
  const make = (): FakeCore => {
    const core = new FakeCore();
    cores.push(core);
    return core;
  };
  let ids = 0;
  const asks: FakeApprovalAsk[] = [];
  const fns = {
    nextAsk: [] as { kind: string; code?: string; secretKey?: string }[],
  };
  return {
    cores,
    asks,
    get nextAsk() {
      return fns.nextAsk;
    },
    set nextAsk(steps) {
      fns.nextAsk = steps;
    },
    newApprovalAsk: (email: string) => {
      if (email === "") throw coreError("invalid_email");
      const ask = new FakeApprovalAsk(fns.nextAsk);
      asks.push(ask);
      return ask;
    },
    newCore: make,
    loadCore: (snapshot) => {
      const core = make();
      const state = JSON.parse(new TextDecoder().decode(snapshot)) as {
        account?: string;
        items: [string, string[]][];
      };
      core.account = state.account;
      core.items_ = new Map(state.items);
      return core;
    },
    newId: () => `device-${String(++ids)}`,
    generatePassword: (length) => "x".repeat(length),
    generatePassphrase: (words) =>
      Array.from({ length: words }, () => "word").join("-"),
    passwordScore: (password) =>
      password === PASSWORD || password === EXPORT_PASSWORD ? 4 : 1,
    breachRangeUrl: (prefix) =>
      `https://api.pwnedpasswords.com/range/${prefix}`,
    detectImport: (name, bytes) => {
      const text = new TextDecoder().decode(bytes);
      if (text.startsWith("url,username,password,totp,extra"))
        return JSON.stringify({ format: "lastpass", needs_password: false });
      if (text.startsWith("{"))
        return JSON.stringify({ format: "bitwarden", needs_password: true });
      if (name.endsWith(".csv"))
        return JSON.stringify({ format: "csv", needs_password: false });
      return undefined;
    },
    importColumns: (bytes) => {
      const header = new TextDecoder().decode(bytes).split(/\r?\n/)[0] ?? "";
      const headers = header.split(",");
      return JSON.stringify({
        headers,
        suggested: { title: headers.indexOf("name"), url: null },
        rows: 1,
      });
    },
    emergencyKit: (email, secretKey, server, accountId, created, paper) =>
      new TextEncoder().encode(
        `%PDF-fake ${[email, secretKey, server ?? "hosted", accountId, created, paper].join(" ")}`,
      ),
  };
}

export class MemoryDisk implements Disk {
  snapshot: Uint8Array | undefined;
  values = new Map<DiskKey, string>();

  getSnapshot(): Promise<Uint8Array | undefined> {
    return Promise.resolve(this.snapshot);
  }

  putSnapshot(bytes: Uint8Array): Promise<void> {
    this.snapshot = bytes;
    return Promise.resolve();
  }

  get(key: DiskKey): Promise<string | undefined> {
    return Promise.resolve(this.values.get(key));
  }

  set(key: DiskKey, value: string): Promise<void> {
    this.values.set(key, value);
    return Promise.resolve();
  }

  remove(keys: DiskKey[]): Promise<void> {
    for (const key of keys) this.values.delete(key);
    return Promise.resolve();
  }
}

/** A frame of the fake page: what its content script would report. */
export interface FakeFrame {
  frameId: number;
  documentId: string;
  url: string;
  fields: FieldInfo[];
}

/** A page the fake browser shows, with scriptable answers. */
export class FakePages implements Pages {
  tab: { id: number; url: string } | undefined = {
    id: 7,
    url: "https://example.com/login",
  };
  frames: FakeFrame[] = [];
  /** Wired to the background's frame report handler by the test. */
  report: (sender: FrameSender, fields: FieldInfo[]) => Promise<unknown> = () =>
    Promise.resolve();
  sent: { target: FrameRef; message: ContentMessage }[] = [];
  fillReply: FillReply = { ok: true };
  /** What a "read" (Save login from this page) finds typed. */
  typed: Typed | null = null;
  /** Documents that navigated away since they reported. */
  gone = new Set<string>();
  popups = 0;
  inlineScripts = false;
  siteAccess = false;
  /** Origins the user granted (the leak check's, D44 §2). */
  hostAccess = new Set<string>();
  injections = 0;

  activeTab() {
    return Promise.resolve(this.tab);
  }

  async inject(tabId: number) {
    this.injections += 1;
    const tab = this.tab;
    if (!tab || tab.id !== tabId) return [];
    for (const frame of this.frames) {
      await this.report(
        {
          tabId,
          frameId: frame.frameId,
          documentId: frame.documentId,
          url: frame.url,
          tabUrl: tab.url,
        },
        frame.fields,
      );
    }
    return this.frames.map((f) => ({
      frameId: f.frameId,
      documentId: f.documentId,
    }));
  }

  send(target: FrameRef, message: ContentMessage): Promise<unknown> {
    if (this.gone.has(target.documentId)) return Promise.resolve(undefined);
    this.sent.push({ target, message });
    if (message.kind === "read") return Promise.resolve(this.typed);
    return Promise.resolve(message.kind === "fill" ? this.fillReply : true);
  }

  openPopup() {
    this.popups += 1;
    return Promise.resolve();
  }

  setInlineScripts(on: boolean) {
    this.inlineScripts = on;
    return Promise.resolve();
  }

  hasSiteAccess() {
    return Promise.resolve(this.siteAccess);
  }

  hasHostAccess(origin: string) {
    return Promise.resolve(this.hostAccess.has(origin));
  }

  passkeyScripts = false;
  windows: string[] = [];
  closed: number[] = [];

  /** Set to make registering the page-world scripts fail, as on old browsers. */
  passkeyScriptsFail = false;

  setPasskeyScripts(on: boolean) {
    if (on && this.passkeyScriptsFail)
      return Promise.reject(new Error("world MAIN"));
    this.passkeyScripts = on;
    return Promise.resolve();
  }

  openWindow(path: string) {
    this.windows.push(path);
    return Promise.resolve(this.windows.length);
  }

  closeWindow(windowId: number) {
    this.closed.push(windowId);
    return Promise.resolve();
  }

  badges = new Map<number, string>();
  globalBadge = "";

  setBadge(tabId: number, text: string) {
    this.badges.set(tabId, text);
    return Promise.resolve();
  }

  setGlobalBadge(text: string) {
    this.globalBadge = text;
    return Promise.resolve();
  }
}

/** A field description with everything empty but what is given. */
export function field(index: number, extra: Partial<FieldInfo>): FieldInfo {
  return {
    index,
    tag: "input",
    type: "text",
    name: "",
    id: "",
    autocomplete: "",
    label: "",
    placeholder: "",
    aria_label: "",
    inputmode: "",
    max_length: null,
    visible: true,
    form: 0,
    ...extra,
  };
}

export interface FakePlatform extends Platform {
  pages: FakePages;
  native: FakeNative;
  disk: MemoryDisk;
  sessionBytes: { value: Uint8Array | undefined };
  alarmsAt: Map<AlarmName, number>;
  clipboardText: { value: string | null };
  requests: { url: string; init: RequestInit }[];
  clock: { now: number };
  /** Set to make every fetch fail, or answer with this status. */
  network: {
    down: boolean;
    status: number;
    body: Uint8Array;
    etag: string | null;
  };
}

/**
 * The Mac's helper (D48 §5): keeps enrolled keys in memory and answers as
 * the real one does; `touchId` is what the next Touch ID prompt does.
 */
export class FakeNative implements NativeHost {
  readonly keys = new Map<string, string>();
  readonly sent: Record<string, unknown>[] = [];
  touchId: "ok" | "cancel" | "changed" = "ok";
  installed = true;

  send(message: Record<string, unknown>): Promise<unknown> {
    this.sent.push(message);
    if (!this.installed) return Promise.reject(new Error("no host"));
    const id = `${String(message.account_id)}|${String(message.device_id)}`;
    const fail = (error: string) => Promise.resolve({ v: 1, ok: false, error });
    if (message.type === "forget") {
      this.keys.delete(id);
      return Promise.resolve({ v: 1, ok: true });
    }
    if (this.touchId === "cancel") return fail("cancelled");
    if (this.touchId === "changed") {
      this.keys.delete(id);
      return fail("changed");
    }
    if (message.type === "enroll") {
      this.keys.set(id, String(message.key));
      return Promise.resolve({ v: 1, ok: true });
    }
    const key = this.keys.get(id);
    if (key === undefined) return fail("unknown");
    return Promise.resolve({ v: 1, ok: true, key });
  }
}

export function fakePlatform(disk = new MemoryDisk()): FakePlatform {
  const sessionBytes: { value: Uint8Array | undefined } = { value: undefined };
  const alarmsAt = new Map<AlarmName, number>();
  const clipboardText: { value: string | null } = { value: null };
  const requests: { url: string; init: RequestInit }[] = [];
  const clock = { now: 1_000_000 };
  const network = {
    down: false,
    status: 200,
    body: new Uint8Array(),
    etag: null as string | null,
  };
  return {
    disk,
    pages: new FakePages(),
    native: new FakeNative(),
    capabilities: { inlineUi: true },
    sessionBytes,
    alarmsAt,
    clipboardText,
    requests,
    clock,
    network,
    session: {
      get: () => Promise.resolve(sessionBytes.value),
      set: (bytes) => {
        sessionBytes.value = bytes;
        return Promise.resolve();
      },
      clear: () => {
        sessionBytes.value = undefined;
        return Promise.resolve();
      },
    },
    alarms: {
      set: (name, atMs) => {
        alarmsAt.set(name, atMs);
        return Promise.resolve();
      },
      clear: (name) => {
        alarmsAt.delete(name);
        return Promise.resolve();
      },
    },
    clipboard: {
      write: (text) => {
        clipboardText.value = text;
        return Promise.resolve();
      },
    },
    fetch: (url, init) => {
      requests.push({ url, init });
      if (network.down) return Promise.reject(new TypeError("Failed to fetch"));
      const headers: Record<string, string> =
        network.etag === null ? {} : { etag: network.etag };
      const body = network.status === 304 ? null : network.body.slice();
      return Promise.resolve(
        new Response(body, { status: network.status, headers }),
      );
    },
    now: () => clock.now,
    locale: "en-GB",
    browser: "Chrome 140",
    random: () => 0.5,
  };
}

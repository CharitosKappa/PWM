/**
 * Test doubles: a scripted core and an in-memory platform. The real core is
 * tested in Rust and, through WASM, in `wasm.test.ts`.
 */
import type { Core, CoreFunctions, CoreStep } from "./core.ts";
import type { AlarmName, Disk, DiskKey, Platform } from "./platform.ts";

export const PASSWORD = "correct horse battery staple";
export const SECRET_KEY = "A3-ABCDEF-GHJKLM-NPQRST-UVWXYZ-23456-789AB";
export const ITEM_PASSWORD = "hunter2-item-password";
export const TOTP_SECRET = "JBSWY3DPEHPK3PXP";

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
  ): CoreStep {
    if (deviceId === "" || platform !== "extension")
      throw coreError("invalid_id");
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
      (j) => JSON.parse(j) as { id: string; totp?: string },
    );
    return item.find((i) => i.id === itemId)?.totp
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

  addLogin(vaultId: string, item: Record<string, unknown>): void {
    const list = this.items_.get(vaultId) ?? [];
    list.push(JSON.stringify(item));
    this.items_.set(vaultId, list);
  }
}

/** Core functions that hand out `FakeCore`s; `cores` keeps every one made. */
export function fakeFunctions(): CoreFunctions & { cores: FakeCore[] } {
  const cores: FakeCore[] = [];
  const make = (): FakeCore => {
    const core = new FakeCore();
    cores.push(core);
    return core;
  };
  let ids = 0;
  return {
    cores,
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
    passwordScore: (password) => (password === PASSWORD ? 4 : 1),
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

export interface FakePlatform extends Platform {
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
    random: () => 0.5,
  };
}

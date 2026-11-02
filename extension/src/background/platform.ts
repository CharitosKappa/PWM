/**
 * What the background needs from the browser, as small interfaces: the real
 * ones wrap `chrome.*` (see `chrome.ts`), the tests use fakes.
 */

/** Durable, extension-private storage (`chrome.storage.local` + IndexedDB). */
export interface Disk {
  getSnapshot(): Promise<Uint8Array | undefined>;
  putSnapshot(bytes: Uint8Array): Promise<void>;
  get(key: DiskKey): Promise<string | undefined>;
  set(key: DiskKey, value: string): Promise<void>;
  remove(keys: DiskKey[]): Promise<void>;
}

/**
 * What is kept on disk (D34): the Secret Key as is (it is designed to live
 * on the device), the tokens only sealed under the account keyset, never
 * the master password or an unlocked key.
 */
export type DiskKey =
  | "email"
  | "secretKey"
  | "sealedTokens"
  | "deviceId"
  | "settings"
  | "rules"
  | "rulesVersion"
  | "rulesEtag";

/** Memory-only storage that survives worker restarts (`chrome.storage.session`). */
export interface SessionStore {
  get(): Promise<Uint8Array | undefined>;
  set(session: Uint8Array): Promise<void>;
  clear(): Promise<void>;
}

export type AlarmName = "autolock" | "clipboard" | "rules" | "sync-retry";

export interface Alarms {
  /** (Re)sets a one-shot alarm at `atMs`. */
  set(name: AlarmName, atMs: number): Promise<void>;
  clear(name: AlarmName): Promise<void>;
}

export interface Clipboard {
  /** Writes `text` (an empty string clears). */
  write(text: string): Promise<void>;
}

export interface Platform {
  disk: Disk;
  session: SessionStore;
  alarms: Alarms;
  clipboard: Clipboard;
  fetch: (url: string, init: RequestInit) => Promise<Response>;
  now: () => number;
  /** 0 ≤ x < 1, for spreading the rules download. */
  random: () => number;
}

import type { ContentMessage } from "../shared/autofill.ts";

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
  | "rulesEtag"
  | "inlineHidden"
  | "inlineIcon"
  | "saveNever";

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

/** One document in a tab, as the browser identifies it (never as the page says). */
export interface FrameRef {
  tabId: number;
  frameId: number;
  documentId: string;
}

/** What the browser says about the sender of a content script message. */
export interface FrameSender extends FrameRef {
  /** The frame's URL. */
  url: string;
  /** The tab's (top page's) URL. */
  tabUrl: string;
}

/** Tabs, frames and content scripts (D35). */
export interface Pages {
  /** The tab the user is looking at, if the extension may see it. */
  activeTab(): Promise<{ id: number; url: string } | undefined>;
  /** Runs the content script in every frame it may reach; returns those frames. */
  inject(tabId: number): Promise<{ frameId: number; documentId: string }[]>;
  /** Sends to exactly this document; undefined when it is gone. */
  send(target: FrameRef, message: ContentMessage): Promise<unknown>;
  openPopup(): Promise<void>;
  /** Registers or removes the content script for the inline icon. */
  setInlineScripts(on: boolean): Promise<void>;
  /** Whether the optional site access for the inline icon is granted. */
  hasSiteAccess(): Promise<boolean>;
  /** The toolbar button's badge on one tab ("" clears it). */
  setBadge(tabId: number, text: string): Promise<void>;
}

export interface Platform {
  disk: Disk;
  session: SessionStore;
  alarms: Alarms;
  clipboard: Clipboard;
  pages: Pages;
  fetch: (url: string, init: RequestInit) => Promise<Response>;
  now: () => number;
  /** 0 ≤ x < 1, for spreading the rules download. */
  random: () => number;
}

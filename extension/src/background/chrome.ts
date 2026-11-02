/**
 * The real `Platform`: `chrome.*` and IndexedDB behind the small interfaces
 * in `platform.ts`.
 */
import type {
  AlarmName,
  Alarms,
  Clipboard,
  Disk,
  DiskKey,
  FrameRef,
  Pages,
  Platform,
  SessionStore,
} from "./platform.ts";
import { fromBase64, toBase64 } from "../shared/bytes.ts";
import type { ContentMessage } from "../shared/autofill.ts";
import { SITE_ACCESS } from "../shared/config.ts";
import type { ClipboardMessage } from "../shared/messages.ts";

const DB_NAME = "pwm";
const DB_STORE = "cache";
const SNAPSHOT_KEY = "snapshot";
const SESSION_KEY = "session";

function request<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => {
      resolve(req.result);
    };
    req.onerror = () => {
      reject(req.error ?? new Error("indexeddb"));
    };
  });
}

/** The cache snapshot in IndexedDB (it can be megabytes), small strings in `chrome.storage.local`. */
export class ChromeDisk implements Disk {
  private db: Promise<IDBDatabase> | null = null;

  constructor(
    private readonly idb: IDBFactory,
    private readonly local: chrome.storage.StorageArea,
  ) {}

  private open(): Promise<IDBDatabase> {
    this.db ??= new Promise((resolve, reject) => {
      const req = this.idb.open(DB_NAME, 1);
      req.onupgradeneeded = () => {
        req.result.createObjectStore(DB_STORE);
      };
      req.onsuccess = () => {
        resolve(req.result);
      };
      req.onerror = () => {
        reject(req.error ?? new Error("indexeddb"));
      };
    });
    return this.db;
  }

  async getSnapshot(): Promise<Uint8Array | undefined> {
    const db = await this.open();
    const value: unknown = await request(
      db.transaction(DB_STORE).objectStore(DB_STORE).get(SNAPSHOT_KEY),
    );
    return value instanceof Uint8Array ? value : undefined;
  }

  async putSnapshot(bytes: Uint8Array): Promise<void> {
    const db = await this.open();
    const tx = db.transaction(DB_STORE, "readwrite");
    tx.objectStore(DB_STORE).put(bytes, SNAPSHOT_KEY);
    await new Promise<void>((resolve, reject) => {
      tx.oncomplete = () => {
        resolve();
      };
      tx.onerror = () => {
        reject(tx.error ?? new Error("indexeddb"));
      };
    });
  }

  async get(key: DiskKey): Promise<string | undefined> {
    const items = await this.local.get(key);
    const value: unknown = items[key];
    return typeof value === "string" ? value : undefined;
  }

  async set(key: DiskKey, value: string): Promise<void> {
    await this.local.set({ [key]: value });
  }

  async remove(keys: DiskKey[]): Promise<void> {
    await this.local.remove(keys);
  }
}

/** `chrome.storage.session`: memory only, gone when the browser closes, not readable by content scripts. */
export class ChromeSession implements SessionStore {
  constructor(private readonly area: chrome.storage.StorageArea) {}

  async get(): Promise<Uint8Array | undefined> {
    const items = await this.area.get(SESSION_KEY);
    const value: unknown = items[SESSION_KEY];
    return typeof value === "string" ? fromBase64(value) : undefined;
  }

  async set(session: Uint8Array): Promise<void> {
    await this.area.set({ [SESSION_KEY]: toBase64(session) });
  }

  async clear(): Promise<void> {
    await this.area.remove(SESSION_KEY);
  }
}

export class ChromeAlarms implements Alarms {
  async set(name: AlarmName, atMs: number): Promise<void> {
    await chrome.alarms.create(name, { when: atMs });
  }

  async clear(name: AlarmName): Promise<void> {
    await chrome.alarms.clear(name);
  }
}

/** MV3 service workers have no clipboard: an offscreen document writes it (D34). */
export class OffscreenClipboard implements Clipboard {
  private creating: Promise<void> | null = null;

  private async ensureDocument(): Promise<void> {
    const url = chrome.runtime.getURL("offscreen.html");
    const contexts = await chrome.runtime.getContexts({
      contextTypes: [chrome.runtime.ContextType.OFFSCREEN_DOCUMENT],
      documentUrls: [url],
    });
    if (contexts.length > 0) return;
    this.creating ??= chrome.offscreen
      .createDocument({
        url: "offscreen.html",
        reasons: [chrome.offscreen.Reason.CLIPBOARD],
        justification: "Copy a password and clear it 30 seconds later",
      })
      .finally(() => {
        this.creating = null;
      });
    await this.creating;
  }

  async write(text: string): Promise<void> {
    await this.ensureDocument();
    const message: ClipboardMessage = {
      target: "offscreen",
      kind: "clipboard",
      text,
    };
    await chrome.runtime.sendMessage(message);
  }
}

const INLINE_SCRIPT_ID = "inline-icon";

/** Tabs, frames and content scripts through `chrome.tabs` and `chrome.scripting`. */
export class ChromePages implements Pages {
  async activeTab(): Promise<{ id: number; url: string } | undefined> {
    const [tab] = await chrome.tabs.query({
      active: true,
      lastFocusedWindow: true,
    });
    // The URL is there only with activeTab or site access.
    if (tab?.id === undefined || tab.url === undefined) return undefined;
    return { id: tab.id, url: tab.url };
  }

  async inject(
    tabId: number,
  ): Promise<{ frameId: number; documentId: string }[]> {
    try {
      const results = await chrome.scripting.executeScript({
        target: { tabId, allFrames: true },
        files: ["content.js"],
      });
      return results.map((r) => ({
        frameId: r.frameId,
        documentId: r.documentId,
      }));
    } catch {
      // A page the extension may not touch (the store, browser pages).
      return [];
    }
  }

  async send(target: FrameRef, message: ContentMessage): Promise<unknown> {
    try {
      return (await chrome.tabs.sendMessage(target.tabId, message, {
        frameId: target.frameId,
        documentId: target.documentId,
      })) as unknown;
    } catch {
      return undefined;
    }
  }

  async openPopup(): Promise<void> {
    try {
      await chrome.action.openPopup();
    } catch {
      // No focused window; the user can click the toolbar button.
    }
  }

  async setInlineScripts(on: boolean): Promise<void> {
    const existing = await chrome.scripting.getRegisteredContentScripts({
      ids: [INLINE_SCRIPT_ID],
    });
    if (on && existing.length === 0) {
      await chrome.scripting.registerContentScripts([
        {
          id: INLINE_SCRIPT_ID,
          matches: SITE_ACCESS,
          js: ["content.js"],
          allFrames: true,
          runAt: "document_idle",
          persistAcrossSessions: true,
        },
      ]);
    } else if (!on && existing.length > 0) {
      await chrome.scripting.unregisterContentScripts({
        ids: [INLINE_SCRIPT_ID],
      });
    }
  }

  hasSiteAccess(): Promise<boolean> {
    return chrome.permissions.contains({ origins: SITE_ACCESS });
  }

  async setPasskeyScripts(on: boolean): Promise<void> {
    const ids = ["passkeys-page", "passkeys-bridge"];
    const existing = await chrome.scripting.getRegisteredContentScripts({
      ids,
    });
    if (existing.length > 0) {
      await chrome.scripting.unregisterContentScripts({
        ids: existing.map((s) => s.id),
      });
    }
    if (!on) return;
    // Before the page's own scripts, so its first WebAuthn call is ours.
    const common = {
      matches: SITE_ACCESS,
      allFrames: true,
      runAt: "document_start" as const,
      persistAcrossSessions: true,
    };
    await chrome.scripting.registerContentScripts([
      {
        id: "passkeys-page",
        js: ["passkey-page.js"],
        world: "MAIN",
        ...common,
      },
      {
        id: "passkeys-bridge",
        js: ["passkey-bridge.js"],
        world: "ISOLATED",
        ...common,
      },
    ]);
  }

  async openWindow(path: string): Promise<number> {
    const win = await chrome.windows.create({
      url: chrome.runtime.getURL(path),
      type: "popup",
      width: 400,
      height: 480,
      focused: true,
    });
    return win?.id ?? -1;
  }

  async closeWindow(windowId: number): Promise<void> {
    try {
      await chrome.windows.remove(windowId);
    } catch {
      // Already closed.
    }
  }

  async setBadge(tabId: number, text: string): Promise<void> {
    try {
      await chrome.action.setBadgeText({ tabId, text });
      if (text !== "") {
        await chrome.action.setBadgeBackgroundColor({
          tabId,
          color: "#2457D6",
        });
      }
    } catch {
      // The tab closed.
    }
  }
}

/** "Edge 140", "Opera 125", "Chrome 140" from a user agent. */
export function browserName(userAgent: string): string {
  for (const [token, name] of [
    ["Edg/", "Edge"],
    ["OPR/", "Opera"],
    ["Chrome/", "Chrome"],
  ] as const) {
    const at = userAgent.indexOf(token);
    if (at >= 0) {
      const major = /^\d+/.exec(userAgent.slice(at + token.length))?.[0] ?? "";
      return `${name} ${major}`.trim();
    }
  }
  return "Other";
}

export function chromePlatform(): Platform {
  return {
    disk: new ChromeDisk(indexedDB, chrome.storage.local),
    session: new ChromeSession(chrome.storage.session),
    alarms: new ChromeAlarms(),
    clipboard: new OffscreenClipboard(),
    pages: new ChromePages(),
    fetch: (url, init) => fetch(url, init),
    now: () => Date.now(),
    browser: browserName(navigator.userAgent),
    random: () => Math.random(),
  };
}

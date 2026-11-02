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
  Platform,
  SessionStore,
} from "./platform.ts";
import { fromBase64, toBase64 } from "../shared/bytes.ts";
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

export function chromePlatform(): Platform {
  return {
    disk: new ChromeDisk(indexedDB, chrome.storage.local),
    session: new ChromeSession(chrome.storage.session),
    alarms: new ChromeAlarms(),
    clipboard: new OffscreenClipboard(),
    fetch: (url, init) => fetch(url, init),
    now: () => Date.now(),
    random: () => Math.random(),
  };
}

import { afterEach, describe, expect, it, vi } from "vitest";
import { ChromeDisk, ChromeSession, OffscreenClipboard } from "./chrome.ts";

/** Just enough `chrome.storage` area. */
function storageArea(): chrome.storage.StorageArea & {
  data: Map<string, unknown>;
} {
  const data = new Map<string, unknown>();
  const area = {
    data,
    get: (key: string) =>
      Promise.resolve(data.has(key) ? { [key]: data.get(key) } : {}),
    set: (items: Record<string, unknown>) => {
      for (const [k, v] of Object.entries(items))
        data.set(k, structuredClone(v));
      return Promise.resolve();
    },
    remove: (keys: string | string[]) => {
      for (const k of Array.isArray(keys) ? keys : [keys]) data.delete(k);
      return Promise.resolve();
    },
  };
  return area as unknown as chrome.storage.StorageArea & {
    data: Map<string, unknown>;
  };
}

/** Just enough IndexedDB: one database, object stores keyed by string, async callbacks. */
function memoryIndexedDb(): {
  factory: IDBFactory;
  stores: Map<string, Map<string, unknown>>;
} {
  const stores = new Map<string, Map<string, unknown>>();
  const later = (f: () => void): void => {
    setTimeout(f, 0);
  };
  const request = <T>(result: () => T) => {
    const req: {
      result?: T;
      onsuccess?: () => void;
      onerror?: () => void;
      error: null;
    } = { error: null };
    later(() => {
      req.result = result();
      req.onsuccess?.();
    });
    return req;
  };
  const db = {
    createObjectStore: (name: string) => {
      stores.set(name, new Map());
    },
    transaction: (name: string) => {
      const tx: {
        oncomplete?: () => void;
        onerror?: () => void;
        error: null;
        objectStore: () => unknown;
      } = {
        error: null,
        objectStore: () => ({
          get: (key: string) =>
            request(() => structuredClone(stores.get(name)?.get(key))),
          put: (value: unknown, key: string) => {
            stores.get(name)?.set(key, structuredClone(value));
            later(() => tx.oncomplete?.());
            return request(() => key);
          },
        }),
      };
      return tx;
    },
  };
  const factory = {
    open: () => {
      const req: {
        result: unknown;
        onupgradeneeded?: () => void;
        onsuccess?: () => void;
        error: null;
      } = {
        result: db,
        error: null,
      };
      later(() => {
        if (stores.size === 0) req.onupgradeneeded?.();
        req.onsuccess?.();
      });
      return req;
    },
  };
  return { factory: factory as unknown as IDBFactory, stores };
}

describe("ChromeDisk", () => {
  it("keeps the snapshot bytes in IndexedDB across restarts", async () => {
    const idb = memoryIndexedDb();
    const local = storageArea();
    expect(
      await new ChromeDisk(idb.factory, local).getSnapshot(),
    ).toBeUndefined();

    await new ChromeDisk(idb.factory, local).putSnapshot(
      new Uint8Array([1, 2, 3]),
    );
    expect(await new ChromeDisk(idb.factory, local).getSnapshot()).toEqual(
      new Uint8Array([1, 2, 3]),
    );
    expect(local.data.size).toBe(0);
  });

  it("keeps small strings in chrome.storage.local", async () => {
    const local = storageArea();
    const disk = new ChromeDisk(memoryIndexedDb().factory, local);
    await disk.set("sealedTokens", "c2VhbGVk");
    await disk.set("email", "ada@example.com");
    expect(await disk.get("sealedTokens")).toBe("c2VhbGVk");
    await disk.remove(["sealedTokens"]);
    expect(await disk.get("sealedTokens")).toBeUndefined();
    expect(await disk.get("email")).toBe("ada@example.com");
  });

  it("ignores a value of the wrong type", async () => {
    const local = storageArea();
    local.data.set("email", 42);
    expect(
      await new ChromeDisk(memoryIndexedDb().factory, local).get("email"),
    ).toBeUndefined();
  });
});

describe("ChromeSession", () => {
  it("round-trips the session bytes and clears them", async () => {
    const area = storageArea();
    const session = new ChromeSession(area);
    const bytes = new Uint8Array(
      Array.from({ length: 300 }, (_, i) => i % 256),
    );
    await session.set(bytes);
    expect(typeof area.data.get("session")).toBe("string");
    expect(await session.get()).toEqual(bytes);
    await session.clear();
    expect(await session.get()).toBeUndefined();
  });
});

describe("OffscreenClipboard", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("creates the offscreen document once and sends it the text", async () => {
    let open = false;
    const created: unknown[] = [];
    const sent: unknown[] = [];
    vi.stubGlobal("chrome", {
      runtime: {
        ContextType: { OFFSCREEN_DOCUMENT: "OFFSCREEN_DOCUMENT" },
        getURL: (path: string) => `chrome-extension://id/${path}`,
        getContexts: () => Promise.resolve(open ? [{}] : []),
        sendMessage: (message: unknown) => {
          sent.push(message);
          return Promise.resolve(true);
        },
      },
      offscreen: {
        Reason: { CLIPBOARD: "CLIPBOARD" },
        createDocument: (options: unknown) => {
          created.push(options);
          open = true;
          return Promise.resolve();
        },
      },
    });
    const clipboard = new OffscreenClipboard();
    await clipboard.write("secret");
    await clipboard.write("");
    expect(created).toEqual([
      {
        url: "offscreen.html",
        reasons: ["CLIPBOARD"],
        justification: expect.any(String) as unknown,
      },
    ]);
    expect(sent).toEqual([
      { target: "offscreen", kind: "clipboard", text: "secret" },
      { target: "offscreen", kind: "clipboard", text: "" },
    ]);
  });
});

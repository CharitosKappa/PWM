/**
 * The manage page's item screens (M4.14, D67): what reaches the page, Show
 * and Copy by place, editing, trash and folders. The same rules as the
 * desktop's `items.rs`; the core is the fake one, the real calls are
 * tested in Rust and through WASM.
 */
import { describe, expect, it } from "vitest";
import { MANAGE_PAGE_KINDS, popupMay } from "../shared/messages.ts";
import type { Request } from "../shared/messages.ts";
import { PASSWORD, fakeFunctions, fakePlatform } from "./fakes.ts";
import type { FakeCore } from "./fakes.ts";
import { editable, hiddenPaths, masked, pointer, subtitle } from "./items.ts";
import { Background, ServiceError } from "./service.ts";

const VAULT = "vault-1";
const DAY = 24 * 60 * 60 * 1000;

/** A login with every kind of hidden value, a passkey and a fields from a newer version. */
function sample(): Record<string, unknown> {
  return {
    id: "item-1",
    v: 1,
    type: "login",
    title: "Mail",
    notes: "n",
    favorite: true,
    folder_id: "folder-1",
    tags: [],
    custom_fields: [
      { name: "PIN", value: "8642", kind: "hidden" },
      { name: "Hint", value: "blue", kind: "text" },
    ],
    created_at: 1,
    updated_at: 2,
    trashed_at: null,
    login: {
      username: "a@example.com",
      password: "s3cret",
      urls: [{ url: "https://mail.example.com", match: "domain" }],
      totp: "otpauth://totp/Mail?secret=JBSWY3DPEHPK3PXP",
      passkeys: [
        {
          credential_id: "AAAA",
          rp_id: "example.com",
          user_handle: "BBBB",
          user_name: "a",
          private_key: "CCCC",
          sign_count: 0,
          created_at: 5,
        },
      ],
    },
    history: [
      {
        saved_at: 9,
        v: 1,
        type: "login",
        title: "Mail",
        login: { username: "a@example.com", password: "older-pw-9" },
      },
    ],
    future_field: { kept: true },
  };
}

const SECRETS = [
  "s3cret",
  "JBSWY3DPEHPK3PXP",
  "8642",
  "older-pw-9",
  "AAAA",
  "BBBB",
  "CCCC",
];

function put(core: FakeCore, item: Record<string, unknown>): void {
  const list = (core.items_.get(VAULT) ?? []).filter(
    (j) => (JSON.parse(j) as { id: unknown }).id !== item.id,
  );
  list.push(JSON.stringify(item));
  core.items_.set(VAULT, list);
}

function stored(core: FakeCore, id: string): Record<string, unknown> {
  const json = core.item(VAULT, id);
  if (json === undefined) throw new Error(`no ${id}`);
  return JSON.parse(json) as Record<string, unknown>;
}

async function unlocked() {
  const platform = fakePlatform();
  const fns = fakeFunctions();
  const background = await Background.start(fns, platform);
  await background.handle({
    kind: "signUp",
    email: "ada@example.com",
    password: PASSWORD,
  });
  const core = (): FakeCore => {
    const last = fns.cores.at(-1);
    if (!last) throw new Error("no core");
    return last;
  };
  put(core(), sample());
  put(core(), {
    id: "folder-2",
    v: 1,
    type: "folder",
    title: "work",
    trashed_at: null,
  });
  put(core(), {
    id: "folder-1",
    v: 1,
    type: "folder",
    title: "Home",
    trashed_at: null,
  });
  put(core(), {
    id: "card-1",
    v: 1,
    type: "card",
    title: "Visa",
    trashed_at: 5,
    card: { number: "4111 1111 1111 1234", security_code: "999" },
  });
  return { background, platform, core };
}

async function errorOf(promise: Promise<unknown>): Promise<string> {
  try {
    await promise;
  } catch (error) {
    if (error instanceof ServiceError) return error.code;
    throw error;
  }
  throw new Error("no error");
}

describe("the masking rules", () => {
  it("lists the hidden places that hold something", () => {
    expect(hiddenPaths(sample())).toEqual([
      "/login/password",
      "/login/totp",
      "/custom_fields/0/value",
      "/history/0/login/password",
    ]);
    expect(
      hiddenPaths({
        type: "card",
        card: { number: "4111 1111 1111 1234", security_code: "" },
      }),
    ).toEqual(["/card/number"]);
  });

  it("never leaves a hidden value or a passkey's key in the detail", () => {
    const shown = JSON.stringify(masked(sample()));
    for (const secret of SECRETS) expect(shown).not.toContain(secret);
    // Everything else stays, fields from newer versions included.
    expect(shown).toContain("blue");
    expect(shown).toContain("future_field");
    expect(shown).toContain("example.com");
  });

  it("makes newer schemas and unknown types read-only", () => {
    expect(editable(sample())).toBe(true);
    expect(editable({ v: 2, type: "login" })).toBe(false);
    expect(editable({ v: 1, type: "wifi" })).toBe(false);
    expect(editable({ type: "login" })).toBe(false);
  });

  it("never shows a whole card number in the list", () => {
    expect(
      subtitle({ type: "card", card: { number: "4111-1111-1111-1234" } }),
    ).toBe("•••• 1234");
    expect(
      subtitle({
        type: "identity",
        identity: { first_name: "Ada", last_name: "Lovelace" },
      }),
    ).toBe("Ada Lovelace");
  });

  it("reads JSON Pointers, escapes included", () => {
    const value = { "a/b": { "c~d": [10, 20] } };
    expect(pointer(value, "/a~1b/c~0d/1")).toBe(20);
    expect(pointer(value, "/a~1b/c~0d/01")).toBeUndefined();
    expect(pointer(value, "/missing")).toBeUndefined();
    expect(pointer(value, "/constructor")).toBeUndefined();
  });
});

describe("the vault and an item", () => {
  it("lists summaries and folders, never a hidden value", async () => {
    const { background } = await unlocked();
    const vault = await background.handle({ kind: "vault" });
    for (const secret of [...SECRETS, "4111", "999"])
      expect(JSON.stringify(vault)).not.toContain(secret);
    expect(vault.folders.map((f) => f.title)).toEqual(["Home", "work"]);
    expect(vault.items).toEqual([
      {
        vaultId: VAULT,
        itemId: "item-1",
        type: "login",
        title: "Mail",
        subtitle: "a@example.com",
        url: "https://mail.example.com",
        favorite: true,
        folderId: "folder-1",
        trashed: false,
        hasPassword: true,
        hasCode: true,
      },
      expect.objectContaining({
        itemId: "card-1",
        subtitle: "•••• 1234",
        trashed: true,
        hasPassword: false,
      }),
    ]);
  });

  it("shows an item with its hidden values left out", async () => {
    const { background } = await unlocked();
    const view = await background.handle({
      kind: "item",
      vaultId: VAULT,
      itemId: "item-1",
    });
    for (const secret of SECRETS)
      expect(JSON.stringify(view)).not.toContain(secret);
    expect(view.hidden).toEqual(hiddenPaths(sample()));
    expect(view.readOnly).toBe(false);
    expect(view.code).toEqual({ code: "287082", secondsLeft: 1 });
    expect(view.passkeys).toEqual([
      { rpId: "example.com", userName: "a", createdAt: 5 },
    ]);
    expect(view.passwordHistory).toEqual([
      { savedAt: 9, path: "/history/0/login/password" },
    ]);
    expect(
      await errorOf(
        background.handle({ kind: "item", vaultId: VAULT, itemId: "nope" }),
      ),
    ).toBe("unknown_item");
  });

  it("shows one hidden value, and only a hidden place", async () => {
    const { background } = await unlocked();
    const reveal = (path: string) =>
      background.handle({
        kind: "reveal",
        vaultId: VAULT,
        itemId: "item-1",
        path,
      });
    expect(await reveal("/login/password")).toBe("s3cret");
    expect(await reveal("/history/0/login/password")).toBe("older-pw-9");
    expect(await reveal("/custom_fields/0/value")).toBe("8642");
    for (const path of [
      "/title",
      "/login/passkeys/0/private_key",
      "/custom_fields/1/value",
      "",
    ])
      expect(await errorOf(reveal(path))).toBe("invalid_request");
  });

  it("copies any field by its place, with the 30-second clear", async () => {
    const { background, platform } = await unlocked();
    await background.handle({
      kind: "copy",
      vaultId: VAULT,
      itemId: "item-1",
      path: "/custom_fields/1/value",
    });
    expect(platform.clipboardText.value).toBe("blue");
    expect(platform.alarmsAt.get("clipboard")).toBe(
      platform.clock.now + 30_000,
    );
    expect(
      await errorOf(
        background.handle({
          kind: "copy",
          vaultId: VAULT,
          itemId: "item-1",
          path: "/nothing/here",
        }),
      ),
    ).toBe("empty");
  });

  it("refuses everything while locked", async () => {
    const { background } = await unlocked();
    await background.handle({ kind: "lock" });
    expect(await errorOf(background.handle({ kind: "vault" }))).toBe("locked");
    expect(
      await errorOf(
        background.handle({
          kind: "reveal",
          vaultId: VAULT,
          itemId: "item-1",
          path: "/login/password",
        }),
      ),
    ).toBe("locked");
  });
});

describe("editing", () => {
  it("gives the editor the item without its history or passkeys", async () => {
    const { background, core } = await unlocked();
    const item = await background.handle({
      kind: "itemForEdit",
      vaultId: VAULT,
      itemId: "item-1",
    });
    expect(item.history).toBeUndefined();
    expect(pointer(item, "/login/passkeys")).toBeUndefined();
    expect(pointer(item, "/login/password")).toBe("s3cret");
    expect(item.future_field).toEqual({ kept: true });
    put(core(), { id: "new-1", v: 2, type: "login", title: "Newer" });
    expect(
      await errorOf(
        background.handle({
          kind: "itemForEdit",
          vaultId: VAULT,
          itemId: "new-1",
        }),
      ),
    ).toBe("read_only_item");
  });

  it("saves an edit and keeps the stored passkeys", async () => {
    const { background, core, platform } = await unlocked();
    const item = await background.handle({
      kind: "itemForEdit",
      vaultId: VAULT,
      itemId: "item-1",
    });
    const login = item.login as Record<string, unknown>;
    login.password = "n3w-password";
    // The page can't change a passkey: what it sends is ignored.
    login.passkeys = [{ rp_id: "evil.example", private_key: "DDDD" }];
    item.history = [{ login: { password: "planted" } }];
    const saved = await background.handle({
      kind: "saveItem",
      vaultId: VAULT,
      item,
    });
    expect(saved).toEqual({ vaultId: VAULT, itemId: "item-1" });
    const after = stored(core(), "item-1");
    expect(pointer(after, "/login/password")).toBe("n3w-password");
    expect(pointer(after, "/login/passkeys")).toEqual(
      pointer(sample(), "/login/passkeys"),
    );
    expect(after.history).toBeUndefined();
    expect(after.future_field).toEqual({ kept: true });
    // Kept on disk for the next start.
    expect(new TextDecoder().decode(platform.disk.snapshot)).toContain(
      "n3w-password",
    );
  });

  it("makes a new item in the first vault, with an id, a version and times", async () => {
    const { background, core, platform } = await unlocked();
    const saved = await background.handle({
      kind: "saveItem",
      vaultId: null,
      item: {
        type: "login",
        title: "Shop",
        login: {
          username: "ada",
          password: "pw",
          urls: [],
          passkeys: [{ private_key: "DDDD" }],
        },
      },
    });
    expect(saved.vaultId).toBe(VAULT);
    const item = stored(core(), saved.itemId);
    expect(item).toMatchObject({
      v: 1,
      created_at: platform.clock.now,
      updated_at: platform.clock.now,
    });
    expect(pointer(item, "/login/passkeys")).toBeUndefined();
    expect(
      await errorOf(
        background.handle({
          kind: "saveItem",
          vaultId: null,
          item: [] as unknown as Record<string, unknown>,
        }),
      ),
    ).toBe("invalid_item");
  });

  it("moves to the trash, back, and deletes for good", async () => {
    const { background, core, platform } = await unlocked();
    const ids = { vaultId: VAULT, itemId: "item-1" };
    await background.handle({ kind: "trashItem", ...ids });
    expect(stored(core(), "item-1").trashed_at).toBe(platform.clock.now);
    await background.handle({ kind: "restoreItem", ...ids });
    expect(stored(core(), "item-1").trashed_at).toBeNull();
    await background.handle({ kind: "deleteItem", ...ids });
    expect(core().item(VAULT, "item-1")).toBeUndefined();
  });

  it("makes folders, and reads one-time codes through the core", async () => {
    const { background, core } = await unlocked();
    const folder = await background.handle({
      kind: "newFolder",
      title: " Bills ",
    });
    expect(stored(core(), folder.itemId)).toMatchObject({
      type: "folder",
      title: "Bills",
      folder: { parent_id: null },
    });
    expect(
      await errorOf(background.handle({ kind: "newFolder", title: "  " })),
    ).toBe("invalid_item");
    expect(
      await background.handle({
        kind: "totpSetup",
        input: "JBSWY3DPEHPK3PXP",
        title: "Mail",
      }),
    ).toBe("otpauth://totp/Mail?secret=JBSWY3DPEHPK3PXP");
    expect(
      await errorOf(
        background.handle({ kind: "totpSetup", input: "nope", title: "Mail" }),
      ),
    ).toBe("invalid_totp");
  });
});

describe("locking and the trash", () => {
  it("tells open pages when the vault locks", async () => {
    const { background, platform } = await unlocked();
    expect(platform.pages.lockNotices).toBe(0);
    await background.handle({ kind: "lock" });
    expect(platform.pages.lockNotices).toBe(1);
  });

  it("empties items trashed for 30 days at unlock (D19)", async () => {
    const { background, core, platform } = await unlocked();
    put(core(), {
      id: "old",
      v: 1,
      type: "note",
      title: "Old",
      trashed_at: platform.clock.now - 31 * DAY,
    });
    put(core(), {
      id: "recent",
      v: 1,
      type: "note",
      title: "Recent",
      trashed_at: platform.clock.now - 29 * DAY,
    });
    await background.handle({ kind: "lock" });
    await background.handle({ kind: "unlock", password: PASSWORD });
    expect(core().item(VAULT, "old")).toBeUndefined();
    expect(core().item(VAULT, "recent")).toBeDefined();
  });
});

describe("which page may ask", () => {
  it("keeps the item screens to the manage page (D67 §1)", () => {
    expect(popupMay({ kind: "items", query: "" })).toBe(true);
    expect(
      popupMay({
        kind: "copy",
        vaultId: VAULT,
        itemId: "i",
        field: "password",
      }),
    ).toBe(true);
    expect(
      popupMay({ kind: "copy", vaultId: VAULT, itemId: "i", path: "/notes" }),
    ).toBe(false);
    for (const kind of MANAGE_PAGE_KINDS)
      expect(popupMay({ kind } as Request)).toBe(false);
  });
});

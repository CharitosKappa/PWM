/**
 * The manage page's item screens (M4.14, D67): the vault's list, an item to
 * show with every hidden value left out, Show and Copy by a value's place,
 * the editor's copy of an item, saving, trash and folders. The same rules
 * as the desktop's backend (`apps/desktop/src-tauri/src/items.rs`).
 *
 * What reaches the page (D67 §1): lists and details never hold a hidden
 * value; `reveal` answers one value after Show; `itemForEdit` answers the
 * whole item, without its history or its passkeys, which never leave the
 * background. The core decrypts, checks every save and keeps the history
 * (D19); this is the item's decrypted JSON, not vault parsing (D67 §4).
 */
import type {
  FolderView,
  ItemJson,
  ItemView,
  VaultRow,
  VaultView,
} from "../shared/messages.ts";
import { errorCode } from "./core.ts";
import type { Core, CoreFunctions } from "./core.ts";
import { ServiceError } from "./errors.ts";

/** The schema this build writes (D18): a newer one is read-only. */
const SCHEMA_VERSION = 1;
const KNOWN_TYPES = ["login", "note", "card", "identity", "folder"];
const FIXED_HIDDEN = [
  "/login/password",
  "/login/totp",
  "/card/number",
  "/card/security_code",
];

type Json = Record<string, unknown>;

export interface ItemsDeps {
  core: () => Core;
  fns: CoreFunctions;
  now: () => number;
  /** Writes the clipboard with the 30-second clear (D34). */
  copy: (text: string) => Promise<void>;
  /** After a change: keep the snapshot, then sync in the background. */
  saved: () => Promise<void>;
}

function isObject(value: unknown): value is Json {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function nonEmpty(value: unknown): boolean {
  return typeof value === "string" && value !== "";
}

/** The value at a JSON Pointer (RFC 6901), or undefined. */
export function pointer(value: unknown, path: string): unknown {
  if (path === "") return value;
  if (!path.startsWith("/")) return undefined;
  let at: unknown = value;
  for (const raw of path.slice(1).split("/")) {
    const key = raw.replace(/~1/g, "/").replace(/~0/g, "~");
    if (Array.isArray(at)) {
      if (!/^(0|[1-9][0-9]*)$/.test(key)) return undefined;
      at = at[Number(key)];
    } else if (isObject(at) && Object.hasOwn(at, key)) {
      at = at[key];
    } else {
      return undefined;
    }
  }
  return at;
}

/** Replaces the string at a pointer `hiddenPaths` listed. */
function blank(item: Json, path: string): void {
  const cut = path.lastIndexOf("/");
  const parent = pointer(item, path.slice(0, cut));
  const key = path.slice(cut + 1);
  if (Array.isArray(parent)) parent[Number(key)] = "";
  else if (isObject(parent)) parent[key] = "";
}

/**
 * The places of an item's hidden values (D43, D67 §1): a login's password
 * and one-time code secret, a card's number and security code, hidden
 * custom fields, and the passwords in its history. Only places that hold
 * something are listed.
 */
export function hiddenPaths(item: unknown): string[] {
  const out = FIXED_HIDDEN.filter((path) => nonEmpty(pointer(item, path)));
  const fields = pointer(item, "/custom_fields");
  if (Array.isArray(fields)) {
    fields.forEach((field: unknown, i) => {
      if (isObject(field) && field.kind === "hidden" && nonEmpty(field.value))
        out.push(`/custom_fields/${String(i)}/value`);
    });
  }
  const history = pointer(item, "/history");
  if (Array.isArray(history)) {
    history.forEach((version: unknown, i) => {
      if (nonEmpty(pointer(version, "/login/password")))
        out.push(`/history/${String(i)}/login/password`);
    });
  }
  return out;
}

/**
 * The item as the detail shows it: without its history, each hidden value
 * as "", and each passkey down to what names it.
 */
export function masked(item: Json): Json {
  const out = structuredClone(item);
  delete out.history;
  for (const path of hiddenPaths(out)) blank(out, path);
  const passkeys = pointer(out, "/login/passkeys");
  if (Array.isArray(passkeys)) {
    const login = out.login as Json;
    login.passkeys = passkeys.map((p: unknown) =>
      isObject(p)
        ? { rp_id: p.rp_id, user_name: p.user_name, created_at: p.created_at }
        : {},
    );
  }
  return out;
}

/** Whether this build may edit the item (D18): its schema and type are known. */
export function editable(item: Json): boolean {
  return (
    typeof item.v === "number" &&
    Number.isInteger(item.v) &&
    item.v >= 0 &&
    item.v <= SCHEMA_VERSION &&
    KNOWN_TYPES.includes(text(item.type))
  );
}

/** The list's line under the title: the username, a card's last 4 digits, or a name. */
export function subtitle(item: Json): string {
  switch (item.type) {
    case "login":
      return text(pointer(item, "/login/username"));
    case "card": {
      const digits = text(pointer(item, "/card/number")).replace(/\D/g, "");
      return digits.length >= 4 ? `•••• ${digits.slice(-4)}` : "";
    }
    case "identity":
      return `${text(pointer(item, "/identity/first_name"))} ${text(
        pointer(item, "/identity/last_name"),
      )}`.trim();
    default:
      return "";
  }
}

function parse(json: string): Json {
  const value: unknown = JSON.parse(json);
  if (!isObject(value)) throw new ServiceError("invalid_item");
  return value;
}

export class Items {
  constructor(private readonly deps: ItemsDeps) {}

  private stored(vaultId: string, itemId: string): Json {
    const json = this.deps.core().item(vaultId, itemId);
    if (json === undefined) throw new ServiceError("unknown_item");
    return parse(json);
  }

  /** The first vault, where new items go. */
  private firstVault(): string {
    const first = this.deps.core().vaultIds()[0];
    if (first === undefined) throw new ServiceError("no_vault");
    return first;
  }

  /** Whether the plan now includes one-time codes (D73 §1). */
  private codesIncluded(): boolean {
    return this.deps.core().codesIncluded(this.deps.now());
  }

  vault(): VaultView {
    const core = this.deps.core();
    const codesIncluded = this.codesIncluded();
    const items: VaultRow[] = [];
    const folders: FolderView[] = [];
    for (const vaultId of core.vaultIds()) {
      for (const json of core.items(vaultId)) {
        const item = parse(json);
        const itemId = text(item.id);
        const trashed =
          item.trashed_at !== null && item.trashed_at !== undefined;
        if (item.type === "folder") {
          if (!trashed)
            folders.push({ vaultId, itemId, title: text(item.title) });
          continue;
        }
        items.push({
          vaultId,
          itemId,
          type: text(item.type),
          title: text(item.title),
          subtitle: subtitle(item),
          url: text(pointer(item, "/login/urls/0/url")),
          favorite: item.favorite === true,
          folderId: typeof item.folder_id === "string" ? item.folder_id : null,
          trashed,
          hasPassword: nonEmpty(pointer(item, "/login/password")),
          hasCode: codesIncluded && nonEmpty(pointer(item, "/login/totp")),
        });
      }
    }
    folders.sort((a, b) =>
      a.title.toLowerCase().localeCompare(b.title.toLowerCase()),
    );
    return { items, folders, codesIncluded };
  }

  item(vaultId: string, itemId: string): ItemView {
    const item = this.stored(vaultId, itemId);
    let code: ItemView["code"] = null;
    let codeNeedsPlan = false;
    try {
      const now = this.deps
        .core()
        .currentCode(vaultId, itemId, this.deps.now());
      if (now?.[0] !== undefined)
        code = { code: now[0], secondsLeft: Number(now[1] ?? 0) };
    } catch (error) {
      // Without a plan that has codes, the screen says so (D73 §1). A code
      // the core can't read shows no code, as on the desktop.
      codeNeedsPlan = errorCode(error) === "plan_required";
    }
    const passkeys = pointer(item, "/login/passkeys");
    const history = pointer(item, "/history");
    return {
      vaultId,
      itemId,
      hidden: hiddenPaths(item),
      readOnly: !editable(item),
      item: masked(item),
      code,
      codeNeedsPlan,
      passkeys: Array.isArray(passkeys)
        ? passkeys.map((p: unknown) => ({
            rpId: text(pointer(p, "/rp_id")),
            userName: text(pointer(p, "/user_name")),
            createdAt: Number(pointer(p, "/created_at") ?? 0) || 0,
          }))
        : [],
      passwordHistory: Array.isArray(history)
        ? history.flatMap((v: unknown, i) =>
            nonEmpty(pointer(v, "/login/password"))
              ? [
                  {
                    savedAt: Number(pointer(v, "/saved_at") ?? 0) || 0,
                    path: `/history/${String(i)}/login/password`,
                  },
                ]
              : [],
          )
        : [],
    };
  }

  /** One hidden value, after Show: only a place `hiddenPaths` lists. */
  reveal(vaultId: string, itemId: string, path: string): string {
    const item = this.stored(vaultId, itemId);
    if (!hiddenPaths(item).includes(path))
      throw new ServiceError("invalid_request");
    return text(pointer(item, path));
  }

  /** Copies any field by its place (D67 §1), with the 30-second clear. */
  async copyPath(vaultId: string, itemId: string, path: string): Promise<void> {
    const item = this.stored(vaultId, itemId);
    await this.deps.copy(text(pointer(item, path)));
  }

  /** The whole item for the editor, without its history and passkeys. */
  itemForEdit(vaultId: string, itemId: string): ItemJson {
    const item = this.stored(vaultId, itemId);
    if (!editable(item)) throw new ServiceError("read_only_item");
    delete item.history;
    if (isObject(item.login)) delete item.login.passkeys;
    return item;
  }

  /**
   * Saves a new item (no `id`) or an edit. The page can't add, change or
   * drop passkeys: the stored ones are always kept. The core checks the
   * item and keeps the previous version in its history (D19).
   */
  async saveItem(
    vaultId: string | null,
    input: unknown,
  ): Promise<{ vaultId: string; itemId: string }> {
    if (!isObject(input)) throw new ServiceError("invalid_item");
    const item = structuredClone(input);
    const vault = vaultId ?? this.firstVault();
    const now = this.deps.now();
    delete item.history;
    let storedPasskeys: unknown;
    if (typeof item.id === "string") {
      const json = this.deps.core().item(vault, item.id);
      if (json !== undefined)
        storedPasskeys = pointer(parse(json), "/login/passkeys");
    } else {
      item.id = this.deps.fns.newId();
      item.v = SCHEMA_VERSION;
      item.created_at = now;
      item.updated_at = now;
    }
    if (isObject(item.login)) {
      if (storedPasskeys === undefined) delete item.login.passkeys;
      else item.login.passkeys = storedPasskeys;
    }
    const itemId = this.deps.core().saveItem(vault, JSON.stringify(item), now);
    await this.deps.saved();
    return { vaultId: vault, itemId };
  }

  async setTrashed(
    vaultId: string,
    itemId: string,
    trashed: boolean,
  ): Promise<void> {
    this.deps.core().setTrashed(vaultId, itemId, trashed, this.deps.now());
    await this.deps.saved();
  }

  /** Deletes an item for good; the screen asks first. */
  async deleteForever(vaultId: string, itemId: string): Promise<void> {
    this.deps.core().deleteItem(vaultId, itemId);
    await this.deps.saved();
  }

  async newFolder(title: string): Promise<{ vaultId: string; itemId: string }> {
    const trimmed = title.trim();
    if (trimmed === "") throw new ServiceError("invalid_item");
    return this.saveItem(null, {
      type: "folder",
      title: trimmed,
      folder: { parent_id: null },
    });
  }

  /** A pasted `otpauth://` URI or bare secret, read by the core (D23). */
  totpSetup(input: string, title: string): string {
    return this.deps.fns.normalizeTotp(input, title);
  }
}

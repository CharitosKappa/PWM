/**
 * The item screens' model (D67): reading and changing an item's decrypted
 * D18 JSON in place, so fields this version doesn't know are kept (D18, as
 * D41 and D52 do in Swift and Kotlin), and the list's sections, search and
 * order (D43 §3). The core checks every save; nothing here decrypts or
 * parses the vault.
 */
import type { FolderView, ItemJson, VaultRow } from "../protocol.ts";

export type ItemType = "login" | "card" | "identity" | "note";

export const ITEM_TYPES: { type: ItemType; label: string; plural: string }[] = [
  { type: "login", label: "Login", plural: "Logins" },
  { type: "card", label: "Card", plural: "Cards" },
  { type: "identity", label: "Identity", plural: "Identities" },
  { type: "note", label: "Note", plural: "Notes" },
];

export type FieldKind = "text" | "hidden" | "url" | "date";

export const FIELD_KINDS: { kind: FieldKind; label: string }[] = [
  { kind: "text", label: "Text" },
  { kind: "hidden", label: "Hidden" },
  { kind: "url", label: "Website" },
  { kind: "date", label: "Date" },
];

/** A new item of `type`, as D18 lays it out; the backend adds its id. */
export function newItem(type: ItemType): ItemJson {
  const base: ItemJson = {
    type,
    title: "",
    notes: "",
    favorite: false,
    folder_id: null,
    tags: [],
    custom_fields: [],
  };
  switch (type) {
    case "login":
      return {
        ...base,
        login: { username: "", password: "", urls: [], totp: null },
      };
    case "card":
      return {
        ...base,
        card: {
          cardholder: "",
          number: "",
          expiry_month: null,
          expiry_year: null,
          security_code: "",
          brand: "",
        },
      };
    case "identity":
      return {
        ...base,
        identity: {
          first_name: "",
          middle_name: "",
          last_name: "",
          email: "",
          phone: "",
          company: "",
          address_lines: [],
          city: "",
          region: "",
          postal_code: "",
          country: "",
        },
      };
    case "note":
      return base;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** The value at `path` (object keys and array indexes), or undefined. */
export function getIn(item: unknown, path: (string | number)[]): unknown {
  let node: unknown = item;
  for (const key of path) {
    if (Array.isArray(node) && typeof key === "number") node = node[key];
    else if (isRecord(node) && typeof key === "string") node = node[key];
    else return undefined;
  }
  return node;
}

export function textAt(item: unknown, path: (string | number)[]): string {
  const value = getIn(item, path);
  return typeof value === "string" ? value : "";
}

/**
 * A copy of `item` with `value` at `path`: every other field, known or
 * not, is kept as it was. Missing objects on the way are made.
 */
export function setIn<T>(
  item: T,
  path: (string | number)[],
  value: unknown,
): T {
  const [key, ...rest] = path;
  if (key === undefined) return value as T;
  if (typeof key === "number") {
    const list = Array.isArray(item) ? [...(item as unknown[])] : [];
    list[key] = setIn(list[key], rest, value);
    return list as T;
  }
  const record: Record<string, unknown> = isRecord(item) ? { ...item } : {};
  record[key] = setIn(record[key], rest, value);
  return record as T;
}

/** "a, b ,c" into ["a", "b", "c"], without empties. */
export function parseTags(text: string): string[] {
  return text
    .split(",")
    .map((t) => t.trim())
    .filter((t) => t !== "");
}

export interface CustomField {
  name: string;
  value: string;
  kind: FieldKind;
}

export function customFields(item: ItemJson): CustomField[] {
  const list = item.custom_fields;
  if (!Array.isArray(list)) return [];
  return list.map((f: unknown) => ({
    name: textAt(f, ["name"]),
    value: textAt(f, ["value"]),
    kind: (FIELD_KINDS.some((k) => k.kind === getIn(f, ["kind"]))
      ? getIn(f, ["kind"])
      : "text") as FieldKind,
  }));
}

/** Adds an empty custom field, keeping the others as they are. */
export function addCustomField(item: ItemJson, kind: FieldKind): ItemJson {
  const list: unknown[] = Array.isArray(item.custom_fields)
    ? (item.custom_fields as unknown[])
    : [];
  return { ...item, custom_fields: [...list, { name: "", value: "", kind }] };
}

export function removeCustomField(item: ItemJson, index: number): ItemJson {
  const list: unknown[] = Array.isArray(item.custom_fields)
    ? (item.custom_fields as unknown[])
    : [];
  return { ...item, custom_fields: list.filter((_, i) => i !== index) };
}

// --- The list ---

/** Where the sidebar points (D43 §2). */
export type Section =
  | { kind: "all" }
  | { kind: "favorites" }
  | { kind: "type"; type: ItemType }
  | { kind: "folder"; folderId: string }
  | { kind: "trash" };

export function sectionLabel(section: Section, folders: FolderView[]): string {
  switch (section.kind) {
    case "all":
      return "All items";
    case "favorites":
      return "Favorites";
    case "type":
      return ITEM_TYPES.find((t) => t.type === section.type)?.plural ?? "";
    case "folder":
      return folders.find((f) => f.itemId === section.folderId)?.title ?? "";
    case "trash":
      return "Trash";
  }
}

function inSection(row: VaultRow, section: Section): boolean {
  if (section.kind === "trash") return row.trashed;
  if (row.trashed) return false;
  switch (section.kind) {
    case "all":
      return true;
    case "favorites":
      return row.favorite;
    case "type":
      return row.type === section.type;
    case "folder":
      return row.folderId === section.folderId;
  }
}

const collator = new Intl.Collator(undefined, { sensitivity: "base" });

/**
 * The rows of `section` matching `query` (D43 §3): a case-insensitive part
 * of the title, the username (or a card's last 4 digits, or a name) or the
 * website, after trimming spaces; ordered by title, then by id.
 */
export function visibleRows(
  rows: VaultRow[],
  section: Section,
  query: string,
): VaultRow[] {
  const needle = query.trim().toLowerCase();
  return rows
    .filter((row) => inSection(row, section))
    .filter(
      (row) =>
        needle === "" ||
        [row.title, row.subtitle, row.url].some((v) =>
          v.toLowerCase().includes(needle),
        ),
    )
    .sort(
      (a, b) =>
        collator.compare(a.title, b.title) || a.itemId.localeCompare(b.itemId),
    );
}

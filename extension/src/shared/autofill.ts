/**
 * Messages between the background, the content script and the inline icon
 * frame (D35). Content scripts and inline frames are untrusted: the
 * background checks every message and never sends them the vault list.
 */

/** One input as the engine reads it (D33): attributes and labels, never its value. */
export interface FieldInfo {
  /** Position in this frame's scan; the content script keeps the element. */
  index: number;
  tag: string;
  type: string;
  name: string;
  id: string;
  autocomplete: string;
  label: string;
  placeholder: string;
  aria_label: string;
  inputmode: string;
  max_length: number | null;
  visible: boolean;
  form: number | null;
}

/** At most this many fields per frame are looked at. */
export const MAX_FIELDS = 200;

/** A login the current page may get. */
export interface Suggestion {
  vaultId: string;
  itemId: string;
  title: string;
  username: string;
  /** The engine's decision (D33): `fill`, `warn_insecure`, `warn_regex`, `confirm`, `refuse_*`. */
  decision: string;
  /** The login in progress on this tab (multi-step). */
  next: boolean;
}

/** Content script → background: this frame's fields. */
export interface FrameReport {
  target: "background";
  from: "content";
  kind: "frame";
  fields: FieldInfo[];
}

/** The answer to a frame report: where to show the inline icon, if anywhere. */
export interface FrameReply {
  token: string | null;
  icons: number[];
}

export interface FillTarget {
  index: number;
  value: string;
}

/** Background → content script. */
export type ContentMessage =
  | { target: "content"; kind: "scan" }
  | { target: "content"; kind: "fill"; fills: FillTarget[]; scroll: boolean }
  | { target: "content"; kind: "menu"; index: number; open: boolean }
  | { target: "content"; kind: "removeIcons" };

export type FillReply = { ok: true } | { ok: false; reason: string };

/** Inline icon frame → background. */
export type InlineRequest = {
  target: "background";
  from: "inline";
  token: string;
  index: number;
} & (
  | { kind: "menu" }
  | { kind: "fill"; vaultId: string; itemId: string }
  | { kind: "close" }
  | { kind: "hide" }
  | { kind: "unlock" }
);

export type InlineMenu =
  { state: "locked" } | { state: "unlocked"; items: Suggestion[] };

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

const str = (v: unknown): v is string =>
  typeof v === "string" && v.length <= 1000;

function isField(value: unknown): value is FieldInfo {
  if (!isObject(value)) return false;
  return (
    typeof value.index === "number" &&
    [
      "tag",
      "type",
      "name",
      "id",
      "autocomplete",
      "label",
      "placeholder",
      "aria_label",
      "inputmode",
    ].every((k) => str(value[k])) &&
    (value.max_length === null || typeof value.max_length === "number") &&
    typeof value.visible === "boolean" &&
    (value.form === null || typeof value.form === "number")
  );
}

export function isFrameReport(value: unknown): value is FrameReport {
  return (
    isObject(value) &&
    value.target === "background" &&
    value.from === "content" &&
    value.kind === "frame" &&
    Array.isArray(value.fields) &&
    value.fields.length <= MAX_FIELDS &&
    value.fields.every(isField)
  );
}

export function isInlineRequest(value: unknown): value is InlineRequest {
  if (
    !isObject(value) ||
    value.target !== "background" ||
    value.from !== "inline"
  )
    return false;
  if (!str(value.token) || typeof value.index !== "number") return false;
  switch (value.kind) {
    case "menu":
    case "close":
    case "hide":
    case "unlock":
      return true;
    case "fill":
      return str(value.vaultId) && str(value.itemId);
    default:
      return false;
  }
}

export function isContentMessage(value: unknown): value is ContentMessage {
  if (!isObject(value) || value.target !== "content") return false;
  switch (value.kind) {
    case "scan":
    case "removeIcons":
      return true;
    case "fill":
      return (
        Array.isArray(value.fills) &&
        value.fills.every(
          (f) =>
            isObject(f) &&
            typeof f.index === "number" &&
            typeof f.value === "string",
        ) &&
        typeof value.scroll === "boolean"
      );
    case "menu":
      return typeof value.index === "number" && typeof value.open === "boolean";
    default:
      return false;
  }
}

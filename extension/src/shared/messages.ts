/**
 * Messages between the popup and the background service worker (D34).
 * The popup never receives keys, passwords or one-time codes: copying goes
 * through the background's clipboard. The only secrets it ever gets are the
 * Secret Key, once, right after sign-up, and the Emergency Kit PDF made from
 * it (D42), to save.
 */

import type { OfferView, Suggestion } from "./autofill.ts";

export type CopyField = "username" | "password" | "code";

export type Request =
  | { kind: "status" }
  | { kind: "signUp"; email: string; password: string }
  | { kind: "signIn"; email: string; password: string; secretKey: string }
  | { kind: "unlock"; password: string }
  | { kind: "lock" }
  | { kind: "signOut" }
  | { kind: "items"; query: string }
  | { kind: "copy"; vaultId: string; itemId: string; field: CopyField }
  | { kind: "copySecretKey" }
  | { kind: "emergencyKit" }
  | { kind: "copyText"; text: string }
  | { kind: "generate"; mode: "password" | "passphrase" }
  | { kind: "score"; password: string; email: string }
  | { kind: "sync" }
  | { kind: "settings" }
  | { kind: "setSettings"; settings: Settings }
  | { kind: "activity" }
  | { kind: "suggestions" }
  | { kind: "fill"; vaultId: string; itemId: string; confirmed: boolean }
  | { kind: "inlineIcon" }
  | { kind: "setInlineIcon"; on: boolean }
  | { kind: "saveOffer" }
  | {
      kind: "acceptOffer";
      itemId: string | null;
      title?: string;
      username?: string;
    }
  | { kind: "dismissOffer"; never: boolean }
  | { kind: "captureNow" }
  | { kind: "passkeys" }
  | { kind: "setPasskeys"; on: boolean }
  | { kind: "reportPreview"; problem: ReportProblem }
  | { kind: "reportSend" };

/** What the user says went wrong (D38 §3); no free text. */
export type ReportProblem =
  | "nothing_filled"
  | "wrong_field"
  | "no_save_offer"
  | "icon_in_the_way"
  | "other";

/** What the popup shows before sending: a summary and the exact JSON. */
export interface ReportPreview {
  site: string;
  forms: number;
  fields: number;
  /** The field kinds the engine saw. */
  kinds: string[];
  json: string;
}

export type Status =
  | { state: "signedOut" }
  | { state: "locked"; email: string }
  | {
      state: "unlocked";
      email: string;
      syncedAt: number | null;
      syncError: string | null;
    };

export interface ItemSummary {
  vaultId: string;
  itemId: string;
  title: string;
  username: string;
  url: string;
  hasPassword: boolean;
  hasCode: boolean;
}

export interface Settings {
  /** The sync server; the hosted one unless self-hosting. */
  serverUrl: string;
  /** 1, 5, 15, 60 or 480 minutes (D34: never "never"). */
  autoLockMinutes: number;
}

export const AUTO_LOCK_CHOICES = [1, 5, 15, 60, 480] as const;

export type Reply<T> = { ok: true; value: T } | { ok: false; error: string };

/** What each request answers with. */
export interface Replies {
  status: Status;
  signUp: { secretKey: string };
  signIn: null;
  unlock: null;
  lock: null;
  signOut: null;
  items: ItemSummary[];
  copy: null;
  copySecretKey: null;
  /** The PDF as base64: runtime messages carry only JSON. */
  emergencyKit: { pdf: string };
  copyText: null;
  generate: string;
  score: number;
  sync: Status;
  settings: Settings;
  setSettings: Settings;
  activity: null;
  suggestions: { site: string; items: Suggestion[] };
  fill: { copiedCode: boolean };
  inlineIcon: boolean;
  setInlineIcon: boolean;
  saveOffer: OfferView | null;
  acceptOffer: null;
  dismissOffer: null;
  captureNow: OfferView;
  passkeys: boolean;
  setPasskeys: boolean;
  reportPreview: ReportPreview;
  reportSend: null;
}

const KINDS: ReadonlySet<string> = new Set<Request["kind"]>([
  "status",
  "signUp",
  "signIn",
  "unlock",
  "lock",
  "signOut",
  "items",
  "copy",
  "copySecretKey",
  "emergencyKit",
  "copyText",
  "generate",
  "score",
  "sync",
  "settings",
  "setSettings",
  "activity",
  "suggestions",
  "fill",
  "inlineIcon",
  "setInlineIcon",
  "saveOffer",
  "acceptOffer",
  "dismissOffer",
  "captureNow",
  "passkeys",
  "setPasskeys",
  "reportPreview",
  "reportSend",
]);

/** A popup message, wrapped so the background can tell it from others. */
export interface PopupMessage {
  target: "background";
  request: Request;
}

/** Only checks the kind: the sender is already known to be the popup. */
export function isPopupMessage(value: unknown): value is PopupMessage {
  if (typeof value !== "object" || value === null) return false;
  const message = value as { target?: unknown; request?: unknown };
  if (
    message.target !== "background" ||
    typeof message.request !== "object" ||
    message.request === null
  ) {
    return false;
  }
  const kind = (message.request as { kind?: unknown }).kind;
  return typeof kind === "string" && KINDS.has(kind);
}

/** Message from the background to the offscreen document. */
export interface ClipboardMessage {
  target: "offscreen";
  kind: "clipboard";
  text: string;
}

export function isClipboardMessage(value: unknown): value is ClipboardMessage {
  if (typeof value !== "object" || value === null) return false;
  const message = value as Partial<ClipboardMessage>;
  return (
    message.target === "offscreen" &&
    message.kind === "clipboard" &&
    typeof message.text === "string"
  );
}

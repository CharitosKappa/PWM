/**
 * Messages between the popup (and the manage page, D44) and the background
 * service worker (D34). The pages never receive keys, passwords or one-time
 * codes: copying goes through the background's clipboard. The only secrets
 * they ever get are the Secret Key, once, right after sign-up, the
 * Emergency Kit PDF made from it (D42), and an export the user asked for
 * with the master password (D24), each to save.
 */

import type { OfferView, Suggestion } from "./autofill.ts";

export type CopyField = "username" | "password" | "code";

export type Request =
  | { kind: "status" }
  | { kind: "signUp"; email: string; password: string }
  | { kind: "signIn"; email: string; password: string; secretKey: string }
  | { kind: "unlock"; password: string }
  /** Touch ID through the PWM app for Mac (D48 §5). */
  | { kind: "unlockTouchId" }
  | { kind: "touchId" }
  | { kind: "setTouchId"; on: boolean }
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
  | { kind: "reportSend" }
  // The manage page (D44).
  | { kind: "devices" }
  | { kind: "renameDevice"; deviceId: string; name: string }
  | { kind: "revokeDevice"; deviceId: string }
  | { kind: "health" }
  | { kind: "checkLeaks" }
  | { kind: "setLeakCheck"; on: boolean }
  // Asking a signed-in device (D46), from the popup while signed out.
  | { kind: "askApproval"; email: string }
  | { kind: "askApprovalPoll" }
  | { kind: "askApprovalCancel" }
  | { kind: "signInApproved"; password: string }
  // Approving a new device (D46), from the manage page.
  | { kind: "approvals" }
  | { kind: "acceptApproval"; id: string }
  | { kind: "completeApproval"; id: string }
  | { kind: "denyApproval"; id: string }
  | {
      kind: "exportVault";
      masterPassword: string;
      format: ExportFormat;
      /** For `pwmx` only. */
      exportPassword: string;
    };

export type ExportFormat = "pwmx" | "json" | "csv";

/** Where asking a signed-in device stands. The Secret Key stays in the background. */
export interface AskView {
  state: "waiting" | "code" | "approved" | "ended";
  /** The code to compare on the signed-in device. */
  code: string | null;
}

/** A request from a new device, as the signed-in one shows it. */
export interface ApprovalView {
  id: string;
  platform: string;
  createdAt: number;
  state: string;
  /** Once this device accepted and the new device revealed its key. */
  code: string | null;
}

/** One of the account's devices; times are Unix ms. */
export interface DeviceView {
  id: string;
  platform: string;
  /** `null` without a name, or when it does not open on this account. */
  name: string | null;
  createdAt: number;
  lastSeenAt: number;
  revokedAt: number | null;
  current: boolean;
}

/** A login the health report names; never its password. */
export interface HealthItem {
  vaultId: string;
  itemId: string;
  title: string;
  username: string;
  /** Its first website, opened by "Change" (D44 §4); "" without one. */
  url: string;
}

export interface HealthView {
  weak: HealthItem[];
  /** Groups of logins sharing a password. */
  reused: HealthItem[][];
  /** `null` until the leak check has run in this unlocked session. */
  leaked: (HealthItem & { count: number })[] | null;
  /** The leak check's switch (D22 §5): on unless turned off. */
  leakCheck: boolean;
}

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
  | {
      state: "locked";
      email: string;
      /** Touch ID unlock is on for this account (D48 §5). */
      touchId: boolean;
    }
  | {
      state: "unlocked";
      email: string;
      syncedAt: number | null;
      syncError: string | null;
      /** Requests from new devices, read at sync (D46 §5). */
      approvals: number;
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
  devices: DeviceView[];
  renameDevice: DeviceView[];
  revokeDevice: DeviceView[];
  health: HealthView;
  checkLeaks: HealthView;
  setLeakCheck: boolean;
  unlockTouchId: null;
  touchId: boolean;
  setTouchId: boolean;
  /** The file as base64, with the name to save it under. */
  exportVault: { file: string; name: string };
  askApproval: AskView;
  askApprovalPoll: AskView;
  askApprovalCancel: null;
  signInApproved: null;
  approvals: ApprovalView[];
  acceptApproval: ApprovalView[];
  completeApproval: ApprovalView[];
  denyApproval: ApprovalView[];
}

const KINDS: ReadonlySet<string> = new Set<Request["kind"]>([
  "status",
  "signUp",
  "signIn",
  "unlock",
  "unlockTouchId",
  "touchId",
  "setTouchId",
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
  "devices",
  "renameDevice",
  "revokeDevice",
  "health",
  "checkLeaks",
  "setLeakCheck",
  "exportVault",
  "askApproval",
  "askApprovalPoll",
  "askApprovalCancel",
  "signInApproved",
  "approvals",
  "acceptApproval",
  "completeApproval",
  "denyApproval",
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

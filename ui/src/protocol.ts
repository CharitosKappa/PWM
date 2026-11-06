/**
 * The protocol between the screens and whatever holds the vault: the
 * extension's background service worker (D34), or the desktop app's Rust
 * backend (D3). The screens never receive keys, and copying goes through
 * the host's clipboard. The secrets they get are the Secret Key, once,
 * right after sign-up, the Emergency Kit PDF made from it (D42), an export
 * the user asked for with the master password (D24), each to save, and on
 * the item screens (D67: the desktop's window, the extension's manage
 * page) exactly two requests' answers: `reveal` (one hidden value, after
 * Show) and `itemForEdit` (the whole item, while the editor is open). Lists
 * and item details never hold a hidden value, and the extension's popup
 * may not send these requests.
 */

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

/** A saved login an update could go to. */
export interface OfferCandidate {
  vaultId: string;
  itemId: string;
  title: string;
  username: string;
}

/** A pending save offer, as the popup and the card show it: never the password. */
export interface OfferView {
  site: string;
  action: "save" | "update";
  username: string;
  /** The title a new login would get. */
  title: string;
  insecure: boolean;
  candidates: OfferCandidate[];
  locked: boolean;
}

export type CopyField = "username" | "password" | "code";

export type Request =
  | { kind: "status" }
  | { kind: "signUp"; email: string; password: string }
  | { kind: "signIn"; email: string; password: string; secretKey: string }
  | { kind: "unlock"; password: string }
  /** After the server refused this browser's session (D50 §5). */
  | { kind: "signInAgain"; password: string }
  /** Touch ID through the PWM app for Mac (D48 §5). */
  | { kind: "unlockTouchId" }
  | { kind: "touchId" }
  | { kind: "setTouchId"; on: boolean }
  | { kind: "lock" }
  | { kind: "signOut" }
  | { kind: "items"; query: string }
  | { kind: "copy"; vaultId: string; itemId: string; field: CopyField }
  /** Any field, by its JSON Pointer in the item (D67 §1). */
  | { kind: "copy"; vaultId: string; itemId: string; path: string }
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
    }
  /** Import (D51 §9): the picked file as base64, never stored. */
  | { kind: "importDetect"; name: string; file: string }
  | {
      kind: "importPrepare";
      file: string;
      format: string;
      /** The export's own password, or "". */
      password: string;
      /** A KeePass key file as base64, or "". */
      keyFile: string;
      /** For a generic CSV: which column holds what. */
      columns: ImportMapping | null;
    }
  | { kind: "importFinish" }
  | { kind: "importCancel" }
  // The item screens (D67): the desktop's window, the extension's manage page.
  | { kind: "vault" }
  | { kind: "item"; vaultId: string; itemId: string }
  | { kind: "reveal"; vaultId: string; itemId: string; path: string }
  | { kind: "itemForEdit"; vaultId: string; itemId: string }
  /** A new item has no `id`; `vaultId` null puts it in the first vault. */
  | { kind: "saveItem"; vaultId: string | null; item: ItemJson }
  | { kind: "trashItem"; vaultId: string; itemId: string }
  | { kind: "restoreItem"; vaultId: string; itemId: string }
  | { kind: "deleteItem"; vaultId: string; itemId: string }
  | { kind: "newFolder"; title: string }
  | { kind: "totpSetup"; input: string; title: string };

/**
 * An item's decrypted D18 JSON, as the editor changes it. Fields this
 * version doesn't know are kept as they are (D18).
 */
export type ItemJson = Record<string, unknown>;

/** One row of the vault's list (D67 §1): never a hidden value. */
export interface VaultRow {
  vaultId: string;
  itemId: string;
  /** `login`, `card`, `identity`, `note`, or a newer type. */
  type: string;
  title: string;
  /** The username, a card's last 4 digits, or a name. */
  subtitle: string;
  /** A login's first website, for search; "" without one. */
  url: string;
  favorite: boolean;
  folderId: string | null;
  trashed: boolean;
  hasPassword: boolean;
  hasCode: boolean;
}

export interface FolderView {
  vaultId: string;
  itemId: string;
  title: string;
}

export interface VaultView {
  items: VaultRow[];
  folders: FolderView[];
}

/** An item to show (D67 §1): every hidden value left out, by its place. */
export interface ItemView {
  vaultId: string;
  itemId: string;
  /** The D18 JSON with each hidden value as "", without its history. */
  item: ItemJson;
  /** JSON Pointers of the values left out, for Show and Copy. */
  hidden: string[];
  /** A newer schema or an unknown type: shown, never edited (D18). */
  readOnly: boolean;
  /** The one-time code now, never its secret. */
  code: { code: string; secondsLeft: number } | null;
  passkeys: { rpId: string; userName: string; createdAt: number }[];
  /** Previous passwords, newest first, each to Show by its place. */
  passwordHistory: { savedAt: number; path: string }[];
}

export type ImportField =
  | "title"
  | "url"
  | "username"
  | "password"
  | "notes"
  | "totp"
  | "folder"
  | "favorite"
  | "tags";

/** A column index per field; null leaves the field out. */
export type ImportMapping = Partial<Record<ImportField, number | null>>;

export interface ImportColumnsView {
  headers: string[];
  suggested: ImportMapping;
  rows: number;
}

export interface ImportDetected {
  /** `bitwarden`, `1password`, ..., `csv`, `pwm`; null when not recognised. */
  format: string | null;
  needsPassword: boolean;
  /** For a generic CSV. */
  columns: ImportColumnsView | null;
}

export interface ImportPreviewView {
  logins: number;
  notes: number;
  cards: number;
  identities: number;
  folders: number;
  trashed: number;
  /** Reasons such as `attachments` or `duplicates`, with counts. */
  leftOut: { reason: string; count: number }[];
}

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
  signInAgain: Status;
  touchId: boolean;
  setTouchId: boolean;
  /** The file as base64, with the name to save it under. */
  exportVault: { file: string; name: string };
  importDetect: ImportDetected;
  importPrepare: ImportPreviewView;
  importFinish: ImportPreviewView;
  importCancel: null;
  vault: VaultView;
  item: ItemView;
  reveal: string;
  itemForEdit: ItemJson;
  saveItem: { vaultId: string; itemId: string };
  trashItem: null;
  restoreItem: null;
  deleteItem: null;
  newFolder: { vaultId: string; itemId: string };
  totpSetup: string;
  askApproval: AskView;
  askApprovalPoll: AskView;
  askApprovalCancel: null;
  signInApproved: null;
  approvals: ApprovalView[];
  acceptApproval: ApprovalView[];
  completeApproval: ApprovalView[];
  denyApproval: ApprovalView[];
}

export const REQUEST_KINDS: ReadonlySet<string> = new Set<Request["kind"]>([
  "status",
  "signUp",
  "signIn",
  "unlock",
  "signInAgain",
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
  "importDetect",
  "importPrepare",
  "importFinish",
  "importCancel",
  "vault",
  "item",
  "reveal",
  "itemForEdit",
  "saveItem",
  "trashItem",
  "restoreItem",
  "deleteItem",
  "newFolder",
  "totpSetup",
  "askApproval",
  "askApprovalPoll",
  "askApprovalCancel",
  "signInApproved",
  "approvals",
  "acceptApproval",
  "completeApproval",
  "denyApproval",
]);

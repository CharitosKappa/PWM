/**
 * One sample of every request, and of the replies the desktop backend
 * answers (D66 §3). The compiler checks them against `protocol.ts`;
 * `protocol-samples.test.ts` keeps `protocol-samples.json` the same, and
 * the desktop backend's Rust test reads that file, so the two sides can't
 * drift. Fake values only.
 */
import type { Replies, Request } from "./protocol.ts";

export const REQUESTS: Request[] = [
  { kind: "status" },
  { kind: "signUp", email: "a@example.com", password: "correct horse battery" },
  {
    kind: "signIn",
    email: "a@example.com",
    password: "correct horse battery",
    secretKey: "A3-TEST00-000000-00000-00000-00000-00000",
  },
  { kind: "unlock", password: "correct horse battery" },
  { kind: "signInAgain", password: "correct horse battery" },
  { kind: "unlockTouchId" },
  { kind: "touchId" },
  { kind: "setTouchId", on: true },
  { kind: "lock" },
  { kind: "signOut" },
  { kind: "items", query: "mail" },
  {
    kind: "copy",
    vaultId: "00000000-0000-4000-8000-000000000001",
    itemId: "00000000-0000-4000-8000-000000000002",
    field: "password",
  },
  { kind: "copySecretKey" },
  { kind: "emergencyKit" },
  { kind: "copyText", text: "a generated password" },
  { kind: "generate", mode: "passphrase" },
  { kind: "score", password: "hunter2", email: "a@example.com" },
  { kind: "sync" },
  { kind: "settings" },
  {
    kind: "setSettings",
    settings: { serverUrl: "http://localhost:8080", autoLockMinutes: 5 },
  },
  { kind: "activity" },
  { kind: "suggestions" },
  {
    kind: "fill",
    vaultId: "00000000-0000-4000-8000-000000000001",
    itemId: "00000000-0000-4000-8000-000000000002",
    confirmed: false,
  },
  { kind: "inlineIcon" },
  { kind: "setInlineIcon", on: true },
  { kind: "saveOffer" },
  { kind: "acceptOffer", itemId: null, title: "Mail" },
  { kind: "dismissOffer", never: false },
  { kind: "captureNow" },
  { kind: "passkeys" },
  { kind: "setPasskeys", on: false },
  { kind: "reportPreview", problem: "nothing_filled" },
  { kind: "reportSend" },
  { kind: "devices" },
  {
    kind: "renameDevice",
    deviceId: "00000000-0000-4000-8000-000000000003",
    name: "Desk",
  },
  { kind: "revokeDevice", deviceId: "00000000-0000-4000-8000-000000000003" },
  { kind: "health" },
  { kind: "checkLeaks" },
  { kind: "setLeakCheck", on: true },
  { kind: "askApproval", email: "a@example.com" },
  { kind: "askApprovalPoll" },
  { kind: "askApprovalCancel" },
  { kind: "signInApproved", password: "correct horse battery" },
  { kind: "approvals" },
  { kind: "acceptApproval", id: "00000000-0000-4000-8000-000000000004" },
  { kind: "completeApproval", id: "00000000-0000-4000-8000-000000000004" },
  { kind: "denyApproval", id: "00000000-0000-4000-8000-000000000004" },
  {
    kind: "exportVault",
    masterPassword: "correct horse battery",
    format: "pwmx",
    exportPassword: "another long password",
  },
  { kind: "importDetect", name: "export.csv", file: "dGl0bGUK" },
  {
    kind: "importPrepare",
    file: "dGl0bGUK",
    format: "csv",
    password: "",
    keyFile: "",
    columns: { title: 0, username: null },
  },
  { kind: "importFinish" },
  { kind: "importCancel" },
];

/** The kinds the desktop backend answers in M7.4; the rest are `unsupported`. */
export const DESKTOP_KINDS: Request["kind"][] = [
  "status",
  "signUp",
  "signIn",
  "unlock",
  "signInAgain",
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
  "askApproval",
  "askApprovalPoll",
  "askApprovalCancel",
  "signInApproved",
];

/** Replies the desktop backend builds itself, with every shape of each. */
export const REPLIES: {
  [K in keyof Replies]?: Replies[K][];
} = {
  status: [
    { state: "signedOut" },
    { state: "locked", email: "a@example.com", touchId: false },
    {
      state: "unlocked",
      email: "a@example.com",
      syncedAt: 1_762_400_000_000,
      syncError: null,
      approvals: 0,
    },
    {
      state: "unlocked",
      email: "a@example.com",
      syncedAt: null,
      syncError: "offline",
      approvals: 0,
    },
  ],
  items: [
    [
      {
        vaultId: "00000000-0000-4000-8000-000000000001",
        itemId: "00000000-0000-4000-8000-000000000002",
        title: "Mail",
        username: "a@example.com",
        url: "https://mail.example.com",
        hasPassword: true,
        hasCode: false,
      },
    ],
  ],
  settings: [{ serverUrl: "https://api.pwm.invalid", autoLockMinutes: 15 }],
  askApproval: [
    { state: "waiting", code: null },
    { state: "code", code: "123456" },
    { state: "approved", code: null },
    { state: "ended", code: null },
  ],
};

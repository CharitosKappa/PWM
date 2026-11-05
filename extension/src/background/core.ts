/**
 * The core (pwm-wasm, D31) as the background uses it. Typed here rather
 * than imported from the generated package, so type checks and unit tests
 * run without a WASM build; `pwm-core` is mapped to the generated module by
 * the bundler.
 */

export interface CoreStep {
  readonly kind: string;
  readonly method?: string;
  readonly path?: string;
  /** Names and values, alternating. */
  readonly headers: string[];
  readonly body?: Uint8Array;
  readonly delayMs: number;
  readonly outcome?: string;
  readonly secretKey?: string;
  readonly vaultId?: string;
  readonly report: ArrayLike<number>;
  /** After `devices`: JSON array of `{id, platform, name, created_at, last_seen_at, revoked_at, current}`. */
  readonly devices?: string;
  /** After `approvals`: JSON array of `{id, platform, created_at, state, code}` (D46). */
  readonly approvals?: string;
}

/** What a new device does next while asking a signed-in one (D46). */
export interface CoreAskStep {
  /** `send`, `wait`, `code`, `approved` or `ended`. */
  readonly kind: string;
  /** After `send`: the request, as a `send` step. */
  request(): CoreStep | undefined;
  readonly delayMs: number;
  readonly code?: string;
  readonly secretKey?: string;
}

/** A new device's side of one approval. */
export interface CoreApprovalAsk {
  firstRequest(): CoreStep | undefined;
  poll(): CoreStep;
  cancel(): CoreStep | undefined;
  receive(status: number, headers: string[], body: Uint8Array): CoreAskStep;
}

/** One leak check (D22 §4): fetch each prefix, add each body, then finish. */
export interface CoreBreachCheck {
  prefixes(): string[];
  addResponse(prefix: string, body: string): void;
  /** JSON array of `{item_id, count}`. */
  finish(): string;
}

export interface Core {
  snapshot(): Uint8Array;
  accountId(): string | undefined;
  isUnlocked(): boolean;
  startSignup(
    email: string,
    password: string,
    deviceId: string,
    platform: string,
    deviceName: string,
  ): CoreStep;
  startLogin(
    email: string,
    password: string,
    secretKey: string,
    deviceId: string,
    platform: string,
    deviceName: string,
  ): CoreStep;
  startSync(): CoreStep;
  /** Ends the session on the server before signing out (D62); unlocked only. Ends with `logged_out`. */
  startSignOut(): CoreStep;
  startEnsureVault(): CoreStep;
  receive(
    status: number,
    headers: string[],
    body: Uint8Array,
    nowMs: number,
  ): CoreStep;
  networkFailed(): CoreStep;
  cancel(): void;
  takeTokens(): string[] | undefined;
  sealTokens(access: string, refresh: string): Uint8Array;
  unlockWithSealedTokens(
    password: string,
    secretKey: string,
    sealedTokens: Uint8Array | undefined,
  ): void;
  exportSession(): Uint8Array;
  /** Touch ID unlock (D48 §5): a 32-byte key, then the blob. */
  exportBrowserUnlock(deviceId: string): Uint8Array;
  unlockBrowser(
    blob: Uint8Array,
    key: Uint8Array,
    deviceId: string,
    sealedTokens: Uint8Array | undefined,
  ): void;
  restoreSession(
    session: Uint8Array,
    sealedTokens: Uint8Array | undefined,
  ): void;
  lock(): void;
  vaultIds(): string[];
  items(vaultId: string): string[];
  currentCode(
    vaultId: string,
    itemId: string,
    nowMs: number,
  ): string[] | undefined;
  setAutofillRules(
    bytes: Uint8Array,
    nowMs: number,
    minVersion: number,
  ): number;
  /** JSON array of matches for the page (and the frame holding the fields), best first (D33). */
  autofillMatches(pageUrl: string, frameUrl: string | undefined): string;
  /** JSON array of field descriptions in, JSON array of `{class, confidence}` out. */
  classifyFields(fieldsJson: string, pageUrl: string): string;
  autofillRecordStep(
    tab: string,
    pageUrl: string,
    itemId: string,
    step: string,
    nowMs: number,
  ): void;
  /** `[itemId, step]` of a login in progress on this tab. */
  autofillNextStep(
    tab: string,
    pageUrl: string,
    nowMs: number,
  ): string[] | undefined;
  autofillForgetTab(tab: string): void;
  /** What to offer after a sign-in (D36), as JSON `{kind, candidates}`. */
  saveOffer(
    pageUrl: string,
    frameUrl: string | undefined,
    username: string,
    password: string,
    newPassword: string | undefined,
  ): string;
  saveLogin(
    vaultId: string,
    pageUrl: string,
    title: string,
    username: string,
    password: string,
    nowMs: number,
  ): string;
  updatePassword(
    vaultId: string,
    itemId: string,
    password: string,
    nowMs: number,
  ): void;
  /** Passkeys (D37): JSON `{kind, rp_id, user_name}`; origins are the browser's. */
  passkeyCreateCheck(
    origin: string,
    topOrigin: string | undefined,
    requestJson: string,
  ): string;
  passkeyCreate(
    vaultId: string,
    origin: string,
    topOrigin: string | undefined,
    requestJson: string,
    nowMs: number,
  ): string;
  /** JSON array of `{vault_id, item_id, credential_id, user_name, title}`. */
  passkeyCandidates(
    origin: string,
    topOrigin: string | undefined,
    requestJson: string,
  ): string;
  passkeyGet(
    origin: string,
    topOrigin: string | undefined,
    requestJson: string,
    vaultId: string,
    itemId: string,
    credentialId: string,
  ): string;
  /** An "Autofill didn't work here" report (D38), as JSON; never values. */
  autofillReport(
    pageUrl: string,
    problem: string,
    framesJson: string,
    extensionVersion: string,
    browser: string,
  ): string;
  /** Saves an item (D18 JSON with its `id`); returns the id. */
  saveItem(vaultId: string, json: string, nowMs: number): string;
  /** Devices (D43 §6): end with `devices` or `device_updated`. */
  startListDevices(): CoreStep;
  startRenameDevice(deviceId: string, name: string): CoreStep;
  startRevokeDevice(deviceId: string): CoreStep;
  /** Weak and reused passwords (D22), as JSON `{weak: [{item_id, score}], reused: [[item_id]]}`. */
  healthReport(): string;
  breachCheck(): CoreBreachCheck;
  /** The master password again; the next `exportFile` may run (D24). */
  authorizeExport(password: string, secretKey: string, nowMs: number): void;
  /** `pwmx` (with `exportPassword`), `json` or `csv`; uses up the grant. */
  exportFile(format: string, exportPassword: string, nowMs: number): Uint8Array;
  /**
   * Reads a file to import into `vaultId` ("" for the first vault) and
   * previews it as JSON `{logins, notes, cards, identities, folders,
   * trashed, left_out: [[reason, count]]}` (D51). `mapping` is JSON or "".
   */
  prepareImport(
    vaultId: string,
    format: string,
    bytes: Uint8Array,
    password: string,
    /** A KeePass key file, or empty. */
    keyFile: Uint8Array,
    mapping: string,
    nowMs: number,
  ): string;
  /** Saves the prepared import; the preview JSON. */
  finishImport(nowMs: number): string;
  cancelImport(): void;
  /** Device approvals (D46), the signed-in side: end with `approvals` or `approval_updated`. */
  startListApprovals(): CoreStep;
  startAcceptApproval(approvalId: string): CoreStep;
  startCompleteApproval(approvalId: string, secretKey: string): CoreStep;
  startDenyApproval(approvalId: string): CoreStep;
}

export interface CoreFunctions {
  newCore(): Core;
  loadCore(snapshot: Uint8Array): Core;
  newId(): string;
  generatePassword(length: number): string;
  generatePassphrase(words: number): string;
  passwordScore(password: string, context: string[]): number;
  /** The Emergency Kit PDF (D42); `server` only when self-hosted. */
  emergencyKit(
    email: string,
    secretKey: string,
    server: string | undefined,
    accountId: string,
    created: string,
    paper: "a4" | "letter",
  ): Uint8Array;
  /** The HIBP range URL for a 5-character prefix (D22 §4). */
  breachRangeUrl(prefix: string): string;
  /** An import file's format as JSON `{format, needs_password}`, or undefined. */
  detectImport(name: string, bytes: Uint8Array): string | undefined;
  /** A CSV's header and suggested columns as JSON `{headers, suggested, rows}`. */
  importColumns(bytes: Uint8Array): string;
  /** A new device's request to a signed-in one (D46). */
  newApprovalAsk(email: string): CoreApprovalAsk;
}

/** The `code` a core error carries (D31), or `internal`. */
export function errorCode(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof error.code === "string"
  ) {
    return error.code;
  }
  return "internal";
}

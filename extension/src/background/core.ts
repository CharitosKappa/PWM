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
  ): CoreStep;
  startSync(): CoreStep;
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
  /** Saves an item (D18 JSON with its `id`); returns the id. */
  saveItem(vaultId: string, json: string, nowMs: number): string;
}

export interface CoreFunctions {
  newCore(): Core;
  loadCore(snapshot: Uint8Array): Core;
  newId(): string;
  generatePassword(length: number): string;
  generatePassphrase(words: number): string;
  passwordScore(password: string, context: string[]): number;
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

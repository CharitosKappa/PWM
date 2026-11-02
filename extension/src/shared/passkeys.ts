/**
 * Passkey messages (D37). The page-world wrapper and the content bridge are
 * untrusted: the background takes the origin from the browser and checks
 * everything in the core.
 */

/** Requests larger than this are refused before parsing. */
export const MAX_REQUEST = 64 * 1024;

/** Content bridge → background: a page called WebAuthn. */
export interface PasskeyCall {
  target: "background";
  from: "content";
  kind: "passkey";
  op: "create" | "get";
  /** The page's options, binary values as base64url (WebAuthn JSON). */
  request: string;
}

/** What goes back to the page: a credential, the browser's turn, or an error name. */
export type PasskeyAnswer =
  | { result: string }
  | { fallback: true }
  | { error: "NotAllowedError" | "SecurityError" | "InvalidStateError" };

/** The confirmation window's view of a request (D37 §4). Never keys. */
export type PasskeyPrompt =
  | { state: "locked"; site: string }
  | { state: "create"; site: string; rpId: string; userName: string }
  | {
      state: "get";
      site: string;
      rpId: string;
      candidates: {
        vaultId: string;
        itemId: string;
        credentialId: string;
        userName: string;
        title: string;
      }[];
    }
  | { state: "gone" };

/** Confirmation window → background. */
export type PasskeyWindowRequest = {
  target: "background";
  from: "passkeyWindow";
  id: string;
} & (
  | { kind: "prompt" }
  | { kind: "unlock"; password: string }
  | { kind: "create" }
  | { kind: "get"; vaultId: string; itemId: string; credentialId: string }
  | { kind: "browser" }
  | { kind: "cancel" }
);

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

const short = (v: unknown): v is string =>
  typeof v === "string" && v.length <= 1000;

export function isPasskeyCall(value: unknown): value is PasskeyCall {
  return (
    isObject(value) &&
    value.target === "background" &&
    value.from === "content" &&
    value.kind === "passkey" &&
    (value.op === "create" || value.op === "get") &&
    typeof value.request === "string" &&
    value.request.length <= MAX_REQUEST
  );
}

export function isPasskeyWindowRequest(
  value: unknown,
): value is PasskeyWindowRequest {
  if (!isObject(value)) return false;
  if (value.target !== "background" || value.from !== "passkeyWindow")
    return false;
  if (!short(value.id)) return false;
  switch (value.kind) {
    case "prompt":
    case "create":
    case "browser":
    case "cancel":
      return true;
    case "unlock":
      return short(value.password);
    case "get":
      return (
        short(value.vaultId) && short(value.itemId) && short(value.credentialId)
      );
    default:
      return false;
  }
}

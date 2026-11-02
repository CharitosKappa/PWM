/** The popup's only way to the vault: messages to the background (D34). */
import type {
  PopupMessage,
  Replies,
  Reply,
  Request,
} from "../shared/messages.ts";

export class RequestError extends Error {
  constructor(readonly code: string) {
    super(code);
  }
}

export async function send<R extends Request>(
  request: R,
): Promise<Replies[R["kind"]]> {
  const message: PopupMessage = { target: "background", request };
  const reply = await chrome.runtime.sendMessage<
    PopupMessage,
    Reply<Replies[R["kind"]]> | undefined
  >(message);
  if (reply === undefined) throw new RequestError("internal");
  if (!reply.ok) throw new RequestError(reply.error);
  return reply.value;
}

const MESSAGES: Record<string, string> = {
  wrong_password: "Wrong master password.",
  invalid_credentials: "Wrong email, master password or Secret Key.",
  invalid_secret_key:
    "That Secret Key is not valid. Check it on your Emergency Kit.",
  weak_password: "Choose a stronger master password.",
  network: "Can't reach the server. Check your connection.",
  offline: "Offline. Changes will sync later.",
  rate_limited: "Too many tries. Wait a minute and try again.",
  account_exists: "There is already an account with this email.",
  server: "The server had a problem. Try again later.",
  device_revoked: "This device was signed out from another device.",
  signed_out: "You were signed out. Sign in again.",
  invalid_server: "Enter an https:// address.",
  empty: "Nothing to copy.",
  signed_in: "Sign out to change the server.",
};

/** Plain words for an error code. */
export function describeCode(code: string): string {
  return MESSAGES[code] ?? "Something went wrong. Try again.";
}

export function describe(error: unknown): string {
  return describeCode(error instanceof RequestError ? error.code : "internal");
}

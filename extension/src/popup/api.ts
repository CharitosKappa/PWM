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
  field_hidden:
    "The field is hidden or covered, so nothing was filled. Copy instead.",
  no_form: "No sign-in form found on this page.",
  refused: "This login is not for this page or frame.",
  page_changed: "The page changed. Try again.",
  no_page: "Open a web page first.",
  needs_confirm: "Check the warning, then choose Fill anyway.",
  no_password: "No password typed on this page.",
  already_saved: "This login is already saved.",
  nothing_to_save: "This login is already saved.",
  no_offer: "Nothing to save on this page now.",
  no_vault: "No vault yet. Sync, then try again.",
  no_report: "Look at the report again, then send it.",
  passkeys_unsupported: "This browser cannot pass passkey requests to PWM yet.",
  no_server_access: "PWM needs permission to reach the server to sign in.",
  signed_in: "Sign out to change the server.",
  no_leak_access:
    "PWM needs permission to reach the leak check. Only hash prefixes are sent.",
  leak_check_off: "Turn on the leak check first.",
  breach_check: "The leak check is not answering. Try again later.",
  this_device: "To leave this device, use Sign out in Settings.",
  invalid_device_name: "Use a shorter name, up to 63 characters.",
  unknown_device: "That device was removed already.",
  weak_export_password: "Choose a stronger export password.",
  export_not_authorized: "Enter your master password to export.",
  export_expired: "Enter your master password again to export.",
  locked: "Unlock first.",
  helper_missing:
    "Touch ID needs the PWM app for Mac, with browser unlock turned on in its Settings.",
  touch_id_off: "Turn on Touch ID in Settings first.",
  touch_id_unavailable: "Touch ID is not set up on this Mac.",
  biometric_cancelled: "Unlock with your master password, or try again.",
  biometric_changed:
    "Touch ID changed. Enter your master password, then turn Touch ID on again.",
  master_password_due:
    "Enter your master password. It's asked every 14 days so you don't forget it.",
};

/** Plain words for an error code. */
export function describeCode(code: string): string {
  return MESSAGES[code] ?? "Something went wrong. Try again.";
}

export function describe(error: unknown): string {
  return describeCode(error instanceof RequestError ? error.code : "internal");
}

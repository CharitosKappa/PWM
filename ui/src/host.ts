/**
 * What the screens need from the app they run in (D3 §2): the extension's
 * popup and manage page, or the desktop app's window. Each sets its host
 * once, before the first render; the screens never call a browser or
 * Tauri API themselves.
 */
import type { Reply, Request } from "./protocol.ts";

/** Access a screen asks for on a click, in `chrome.permissions` terms. */
export interface Access {
  origins?: string[];
  /** The only optional permission the screens ask for: Touch ID's helper (D48 §5). */
  permissions?: "nativeMessaging"[];
}

export interface Host {
  /** One request to whatever holds the vault; undefined when nothing answered. */
  send(request: Request): Promise<Reply<unknown> | undefined>;
  /**
   * Asks for access on the click that called it: it must be the first thing
   * the click does (D34, D44). True when granted, or when this host needs
   * no permission for it.
   */
  requestAccess(access: Access): Promise<boolean>;
  /** Gives access back once nothing uses it. */
  releaseAccess(access: Access): Promise<void>;
  /** Opens the manage page (D44), at a section when one is given. */
  openManage(section?: "devices"): void;
  /** The inline icon and the save card on pages (D35, D40 §2). */
  readonly inlineUi: boolean;
  /** Whether signing in first needs the user's yes to reach the server (D40 §1). */
  readonly asksServerAccess: boolean;
}

let current: Host | undefined;

/** Sets the host, once, before the first render. */
export function setHost(host: Host): void {
  current = host;
}

export function host(): Host {
  if (current === undefined) throw new Error("no host: call setHost first");
  return current;
}

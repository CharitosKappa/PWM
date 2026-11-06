/**
 * Messages between the popup (and the manage page, D44) and the background
 * service worker (D34). The requests and replies themselves are the shared
 * screens' protocol (D3), in `@pwm/ui`.
 */

import { REQUEST_KINDS } from "@pwm/ui/protocol.ts";
import type { Request } from "@pwm/ui/protocol.ts";

export * from "@pwm/ui/protocol.ts";

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
  return typeof kind === "string" && REQUEST_KINDS.has(kind);
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

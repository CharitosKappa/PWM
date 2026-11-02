/**
 * The isolated-world bridge (D37 §1): carries passkey calls from the
 * page-world wrapper to the background and answers back. It reads only
 * messages this window posted to itself; the background takes the origin
 * from the browser, not from here.
 */
import { MAX_REQUEST } from "../shared/passkeys.ts";
import type { PasskeyAnswer } from "../shared/passkeys.ts";
import type { Reply } from "../shared/messages.ts";

const marker = "pwmPasskeyBridge";

declare global {
  // Set once per frame, so a second injection does not answer twice.
  var pwmPasskeyBridge: true | undefined;
}

if (!globalThis[marker]) {
  globalThis[marker] = true;
  window.addEventListener("message", (event: MessageEvent) => {
    if (event.source !== window) return;
    const data = event.data as {
      source?: unknown;
      id?: unknown;
      op?: unknown;
      request?: unknown;
    };
    if (data.source !== "pwm-passkey" || typeof data.id !== "string") return;
    if (data.op !== "create" && data.op !== "get") return;
    if (typeof data.request !== "string" || data.request.length > MAX_REQUEST)
      return;
    const id = data.id;
    const answer = (value: PasskeyAnswer): void => {
      window.postMessage(
        { source: "pwm-passkey-reply", id, answer: value },
        "*",
      );
    };
    chrome.runtime
      .sendMessage({
        target: "background",
        from: "content",
        kind: "passkey",
        op: data.op,
        request: data.request,
      })
      .then((reply: Reply<PasskeyAnswer> | undefined) => {
        answer(reply?.ok ? reply.value : { fallback: true });
      })
      .catch(() => {
        // The extension went away: the browser handles it.
        answer({ fallback: true });
      });
  });
}

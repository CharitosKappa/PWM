/**
 * The offscreen document's only job: write the clipboard for the background
 * (D34). Never logs what it writes.
 */
import { writeClipboard } from "../shared/clipboard.ts";
import { isClipboardMessage } from "../shared/messages.ts";

export { writeClipboard };

if (typeof chrome !== "undefined" && typeof chrome.runtime !== "undefined") {
  chrome.runtime.onMessage.addListener(
    (message: unknown, sender, sendResponse: (done: boolean) => void) => {
      if (sender.id !== chrome.runtime.id || !isClipboardMessage(message))
        return false;
      writeClipboard(document, message.text);
      sendResponse(true);
      return false;
    },
  );
}

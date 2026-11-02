/**
 * The offscreen document's only job: write the clipboard for the background
 * (D34). Never logs what it writes.
 */
import { isClipboardMessage } from "../shared/messages.ts";

/** Writes `text` through a copy event, which also works for "" (clearing). */
export function writeClipboard(doc: Document, text: string): void {
  const onCopy = (event: ClipboardEvent): void => {
    event.clipboardData?.setData("text/plain", text);
    event.preventDefault();
  };
  doc.addEventListener("copy", onCopy);
  try {
    // The only clipboard API an unfocused offscreen document has.
    // eslint-disable-next-line @typescript-eslint/no-deprecated
    doc.execCommand("copy");
  } finally {
    doc.removeEventListener("copy", onCopy);
  }
}

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

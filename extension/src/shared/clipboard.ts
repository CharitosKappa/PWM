/**
 * Writing the clipboard from a document without focus: the offscreen page
 * on Chromium, the background page on Firefox (D34, D40). Never logs what
 * it writes.
 */

/** Writes `text` through a copy event, which also works for "" (clearing). */
export function writeClipboard(doc: Document, text: string): void {
  const onCopy = (event: ClipboardEvent): void => {
    event.clipboardData?.setData("text/plain", text);
    event.preventDefault();
  };
  doc.addEventListener("copy", onCopy);
  try {
    // The only clipboard API an unfocused document has.
    // eslint-disable-next-line @typescript-eslint/no-deprecated
    doc.execCommand("copy");
  } finally {
    doc.removeEventListener("copy", onCopy);
  }
}

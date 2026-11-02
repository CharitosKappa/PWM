import { describe, expect, it } from "vitest";
import { writeClipboard } from "../offscreen/index.ts";
import { fromBase64, toBase64 } from "./bytes.ts";
import { isClipboardMessage, isPopupMessage } from "./messages.ts";

describe("message guards", () => {
  it("accepts popup requests by kind only", () => {
    expect(
      isPopupMessage({ target: "background", request: { kind: "status" } }),
    ).toBe(true);
    expect(
      isPopupMessage({ target: "background", request: { kind: "exportKeys" } }),
    ).toBe(false);
    expect(
      isPopupMessage({ target: "offscreen", request: { kind: "status" } }),
    ).toBe(false);
    expect(isPopupMessage({ kind: "status" })).toBe(false);
    expect(isPopupMessage(null)).toBe(false);
  });

  it("accepts clipboard messages with text", () => {
    expect(
      isClipboardMessage({ target: "offscreen", kind: "clipboard", text: "" }),
    ).toBe(true);
    expect(isClipboardMessage({ target: "offscreen", kind: "clipboard" })).toBe(
      false,
    );
    expect(
      isClipboardMessage({
        target: "background",
        kind: "clipboard",
        text: "x",
      }),
    ).toBe(false);
  });
});

describe("base64", () => {
  it("round-trips every byte value", () => {
    const bytes = new Uint8Array(Array.from({ length: 256 }, (_, i) => i));
    expect(fromBase64(toBase64(bytes))).toEqual(bytes);
  });
});

describe("offscreen clipboard", () => {
  it("writes the text through a copy event, including an empty one", () => {
    const written: string[] = [];
    let listener: ((event: ClipboardEvent) => void) | null = null;
    const doc = {
      addEventListener: (_: string, l: (event: ClipboardEvent) => void) => {
        listener = l;
      },
      removeEventListener: () => {
        listener = null;
      },
      execCommand: () => {
        const event = {
          clipboardData: {
            setData: (_type: string, text: string) => {
              written.push(text);
            },
          },
          preventDefault: () => undefined,
        } as unknown as ClipboardEvent;
        listener?.(event);
        return true;
      },
    } as unknown as Document;
    writeClipboard(doc, "secret");
    writeClipboard(doc, "");
    expect(written).toEqual(["secret", ""]);
    expect(listener).toBeNull();
  });
});

/**
 * The content script (D35): describes this frame's fields to the
 * background (never their values), sets the values the background sends
 * after a user action, and places the inline icon frames. It runs in an
 * isolated world; it keeps no values and never sees the vault list.
 */
import { isContentMessage, MAX_FIELDS } from "../shared/autofill.ts";
import type {
  ContentMessage,
  FieldInfo,
  FillReply,
  FillTarget,
  FrameReply,
} from "../shared/autofill.ts";
import type { Reply } from "../shared/messages.ts";
import { fieldProblem, samplePoints } from "./checks.ts";
import type { FieldState, StyleInfo } from "./checks.ts";
import { Capturer, readTyped } from "./capture.ts";
import { OfferCard } from "./card.ts";
import { looksVisible } from "./dom.ts";
import { InlineIcons } from "./icons.ts";

const SKIPPED_TYPES = new Set([
  "hidden",
  "submit",
  "button",
  "reset",
  "image",
  "file",
  "checkbox",
  "radio",
  "range",
  "color",
]);

/** Every input in the document and its open shadow roots, in document order. */
function findInputs(
  root: Document | ShadowRoot,
  out: HTMLInputElement[],
): void {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (out.length >= MAX_FIELDS) return;
    const el = node as Element;
    if (el instanceof HTMLInputElement && !SKIPPED_TYPES.has(el.type))
      out.push(el);
    if (el.shadowRoot) findInputs(el.shadowRoot, out);
  }
}

function text(value: string | null | undefined, max = 100): string {
  return (value ?? "").replace(/\s+/g, " ").trim().slice(0, max);
}

/** The engine's description of a field (D33): attributes and labels only. */
function describe(
  el: HTMLInputElement,
  index: number,
  forms: HTMLFormElement[],
): FieldInfo {
  let form: number | null = null;
  if (el.form) {
    if (!forms.includes(el.form)) forms.push(el.form);
    form = forms.indexOf(el.form);
  }
  return {
    index,
    tag: "input",
    type: el.type.toLowerCase(),
    name: text(el.name),
    id: text(el.id),
    autocomplete: text(el.getAttribute("autocomplete")),
    label: text(el.labels?.[0]?.textContent),
    placeholder: text(el.placeholder),
    aria_label: text(el.getAttribute("aria-label")),
    inputmode: text(el.getAttribute("inputmode")),
    max_length: el.maxLength >= 0 ? el.maxLength : null,
    visible: looksVisible(el),
    form,
  };
}

/** The topmost element at a point, looking into open shadow roots. */
function topAt(x: number, y: number): Element | null {
  let top = document.elementFromPoint(x, y);
  while (top?.shadowRoot) {
    const inner = top.shadowRoot.elementFromPoint(x, y);
    if (!inner || inner === top) break;
    top = inner;
  }
  return top;
}

function styleChain(el: Element): StyleInfo[] {
  const out: StyleInfo[] = [];
  let node: Element | null = el;
  while (node) {
    const s = getComputedStyle(node);
    out.push({
      display: s.display,
      visibility: s.visibility,
      opacity: s.opacity,
      filter: s.filter,
      clipPath: s.clipPath,
    });
    const root = node.getRootNode();
    node =
      node.parentElement ?? (root instanceof ShadowRoot ? root.host : null);
  }
  return out;
}

/**
 * IntersectionObserver v2 (D35 §4): is the field visible to the user,
 * including any covering in parent frames? Undefined where unsupported.
 */
function visibleV2(el: Element): Promise<boolean | undefined> {
  if (!("isVisible" in IntersectionObserverEntry.prototype)) {
    return Promise.resolve(undefined);
  }
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      observer.disconnect();
      resolve(false);
    }, 1000);
    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries.at(-1) as
          (IntersectionObserverEntry & { isVisible?: boolean }) | undefined;
        clearTimeout(timer);
        observer.disconnect();
        resolve(entry?.isVisible === true);
      },
      { trackVisibility: true, delay: 100 } as IntersectionObserverInit,
    );
    observer.observe(el);
  });
}

async function measure(el: HTMLInputElement): Promise<FieldState> {
  const r = el.getBoundingClientRect();
  const rect = { left: r.left, top: r.top, width: r.width, height: r.height };
  const onTop = samplePoints(rect).map(([x, y]) => {
    const top = topAt(x, y);
    return (
      top === el || (top instanceof HTMLLabelElement && top.control === el)
    );
  });
  return {
    connected: el.isConnected,
    disabled: el.disabled,
    readOnly: el.readOnly,
    rect,
    viewport: { width: window.innerWidth, height: window.innerHeight },
    styles: styleChain(el),
    onTop,
    visibleV2: await visibleV2(el),
    topFrame: window === window.top,
  };
}

/** Sets a value the way typing would, so page frameworks see it. */
function setValue(el: HTMLInputElement, value: string): void {
  el.focus();
  // The prototype setter: a page may have replaced the element's own.
  const descriptor = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  );
  // Taken off the prototype on purpose, and called with the element.
  // eslint-disable-next-line @typescript-eslint/unbound-method
  const set = descriptor?.set;
  if (set) set.call(el, value);
  else el.value = value;
  el.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
}

class Content {
  private inputs: HTMLInputElement[] = [];
  private readonly icons = new InlineIcons(() => this.inputs);
  private readonly card = new OfferCard();
  private readonly capturer = new Capturer(
    () => this.inputs,
    (message) => {
      chrome.runtime.sendMessage(message).catch(() => {
        // Orphaned after an extension update.
      });
    },
  );

  /** Scans and tells the background; shows the inline icon if it says so. */
  report(): void {
    this.inputs = [];
    findInputs(document, this.inputs);
    const forms: HTMLFormElement[] = [];
    const fields = this.inputs.map((el, i) => describe(el, i, forms));
    chrome.runtime
      .sendMessage({
        target: "background",
        from: "content",
        kind: "frame",
        fields,
      })
      .then((reply: Reply<FrameReply> | undefined) => {
        this.capturer.setWatch(reply?.ok ? reply.value.watch : null);
        if (reply?.ok && reply.value.token !== null) {
          this.icons.show(reply.value.token, reply.value.icons);
        } else {
          this.icons.removeAll();
        }
      })
      .catch(() => {
        // The extension was updated or removed; this script is orphaned.
      });
  }

  async fill(fills: FillTarget[], scroll: boolean): Promise<FillReply> {
    const targets = fills.map((f) => ({
      el: this.inputs[f.index],
      value: f.value,
    }));
    if (targets.some((t) => t.el === undefined))
      return { ok: false, reason: "gone" };
    // Our own icons and card overlap the page; the checks would count them.
    const restoreIcons = this.icons.hideWhile();
    const restoreCard = this.card.hideWhile();
    try {
      // Check every field first: fill all of them or none.
      for (const { el } of targets) {
        if (!el) return { ok: false, reason: "gone" };
        let problem = fieldProblem(await measure(el));
        if (problem === "off_screen" && scroll) {
          el.scrollIntoView({ block: "center", inline: "nearest" });
          await new Promise((r) => requestAnimationFrame(r));
          problem = fieldProblem(await measure(el));
        }
        if (problem !== null) return { ok: false, reason: problem };
      }
      for (const { el, value } of targets) if (el) setValue(el, value);
      return { ok: true };
    } finally {
      restoreIcons();
      restoreCard();
    }
  }

  handle(message: ContentMessage, respond: (reply: unknown) => void): boolean {
    switch (message.kind) {
      case "scan":
        this.report();
        respond(true);
        return false;
      case "fill":
        void this.fill(message.fills, message.scroll).then(respond);
        return true;
      case "menu":
        this.icons.setOpen(message.index, message.open);
        respond(true);
        return false;
      case "removeIcons":
        this.icons.removeAll();
        respond(true);
        return false;
      case "read":
        // "Save login from this page": the user asked, so read now.
        respond(readTyped(this.inputs, message.watch));
        return false;
      case "showOffer":
        if (window === window.top) this.card.show(message.token);
        respond(true);
        return false;
      case "hideOffer":
        this.card.hide();
        respond(true);
        return false;
    }
  }
}

declare global {
  // Kept on the isolated world's global, so a second injection reuses it.
  var pwmContent: Content | undefined;
}

if (globalThis.pwmContent) {
  globalThis.pwmContent.report();
} else {
  const content = new Content();
  globalThis.pwmContent = content;
  chrome.runtime.onMessage.addListener((message: unknown, sender, respond) => {
    if (sender.id !== chrome.runtime.id || !isContentMessage(message))
      return false;
    return content.handle(message, respond);
  });
  content.report();
}

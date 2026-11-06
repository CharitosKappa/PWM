/** Small DOM helpers shared by the content script's parts. */
import { unreachable } from "./checks.ts";
import type { Reach } from "./checks.ts";

/** The element and its ancestors' accessibility state, through shadow roots. */
function reachChain(el: Element): Reach[] {
  const out: Reach[] = [];
  for (let node: Element | null = el; node;) {
    out.push({
      ariaHidden: node.getAttribute("aria-hidden"),
      inert: node instanceof HTMLElement && node.inert,
    });
    const root = node.getRootNode();
    node =
      node.parentElement ?? (root instanceof ShadowRoot ? root.host : null);
  }
  return out;
}

/**
 * Whether a field looks visible enough to be a real one. Honeypots have
 * no size or are parked outside the page (left: -9999px); decoys for
 * browsers' own password managers are hidden from people with
 * `aria-hidden` or `inert` (D65 §1).
 */
export function looksVisible(el: HTMLElement): boolean {
  const rect = el.getBoundingClientRect();
  const style = getComputedStyle(el);
  return (
    rect.width > 1 &&
    rect.height > 1 &&
    rect.right + window.scrollX > 0 &&
    rect.bottom + window.scrollY > 0 &&
    style.visibility === "visible" &&
    Number(style.opacity) > 0 &&
    !unreachable(reachChain(el))
  );
}

/** The element an event really started on, through open shadow roots. */
export function origin(event: Event): Element | null {
  const first = event.composedPath()[0];
  return first instanceof Element ? first : null;
}

/** Small DOM helpers shared by the content script's parts. */

/**
 * Whether a field looks visible enough to be a real one. Honeypots have
 * no size or are parked outside the page (left: -9999px).
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
    Number(style.opacity) > 0
  );
}

/** The element an event really started on, through open shadow roots. */
export function origin(event: Event): Element | null {
  const first = event.composedPath()[0];
  return first instanceof Element ? first : null;
}

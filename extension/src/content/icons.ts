/**
 * The inline icon (D35 §3): a positioned host with a closed shadow root
 * holding the extension's own frame (`inline.html`). The page cannot read
 * or script that frame; the frame checks its own visibility before taking
 * a click. Positioned absolutely, so the page's layout never moves. If
 * the page moves an icon (by script or by CSS), it is put back and its
 * frame restarts the wait before it takes a click (D39: a page must not
 * slide the icon under the cursor).
 */

const ICON = 22;
const MENU = { width: 280, height: 240 };
const Z_TOP = "2147483647";

interface Icon {
  index: number;
  host: HTMLElement;
  frame: HTMLIFrameElement;
  open: boolean;
  /** Where we put it, in page coordinates. */
  placed: { left: number; top: number } | null;
  stunned: ReturnType<typeof setTimeout> | undefined;
}

/** How often the icons are checked for moves the page made by CSS. */
const WATCH_MS = 100;
/** How long a moved icon lets clicks through to the page. */
const STUN_MS = 500;

export class InlineIcons {
  private icons: Icon[] = [];
  private listening = false;
  private frameRequested = false;
  /** Fills being checked: icons stay off the page, even if a report shows them. */
  private suspended = 0;

  constructor(private readonly inputs: () => HTMLInputElement[]) {}

  show(token: string, indexes: number[]): void {
    const keep = new Set(indexes);
    for (const icon of this.icons.filter((i) => !keep.has(i.index)))
      icon.host.remove();
    this.icons = this.icons.filter((i) => keep.has(i.index));
    for (const index of indexes) {
      if (this.icons.some((i) => i.index === index)) continue;
      const host = document.createElement("pwm-inline");
      host.style.cssText = `all: initial; position: absolute; z-index: ${Z_TOP}; display: none;`;
      const shadow = host.attachShadow({ mode: "closed" });
      const frame = document.createElement("iframe");
      // `use_dynamic_url`: the URL changes per session, so pages cannot detect us.
      frame.src = `${chrome.runtime.getURL("inline.html")}#${encodeURIComponent(token)}:${String(index)}`;
      frame.setAttribute("allowtransparency", "true");
      frame.style.cssText =
        "all: initial; display: block; width: 100%; height: 100%; border: 0; color-scheme: normal;";
      shadow.append(frame);
      document.documentElement.append(host);
      const icon: Icon = {
        index,
        host,
        frame,
        open: false,
        placed: null,
        stunned: undefined,
      };
      this.icons.push(icon);
      // A script that moves the icon is seen at once (a microtask after its
      // change, before the next click); stylesheet moves by the poll.
      new MutationObserver(() => {
        this.watch();
      }).observe(host, {
        attributes: true,
        attributeFilter: ["style", "class"],
      });
    }
    this.listen();
    this.place();
  }

  setOpen(index: number, open: boolean): void {
    for (const icon of this.icons) icon.open = icon.index === index && open;
    this.place();
  }

  /**
   * Takes the icons off the page until the returned function puts them
   * back. Our own icon overlaps its field, and the visibility checks would
   * count it as covering the field.
   */
  hideWhile(): () => void {
    this.suspended += 1;
    for (const icon of this.icons) icon.host.style.display = "none";
    return () => {
      this.suspended -= 1;
      this.place();
    };
  }

  removeAll(): void {
    for (const icon of this.icons) icon.host.remove();
    this.icons = [];
  }

  private listen(): void {
    if (this.listening) return;
    this.listening = true;
    const later = (): void => {
      if (this.frameRequested) return;
      this.frameRequested = true;
      requestAnimationFrame(() => {
        this.frameRequested = false;
        this.place();
      });
    };
    window.addEventListener("scroll", later, { passive: true, capture: true });
    window.addEventListener("resize", later, { passive: true });
    setInterval(() => {
      this.watch();
    }, WATCH_MS);
    // A click anywhere else closes an open menu.
    document.addEventListener(
      "mousedown",
      (event) => {
        if (!this.icons.some((i) => i.host === event.target))
          this.setOpen(-1, false);
      },
      { capture: true, passive: true },
    );
  }

  /** Puts back an icon the page moved, and tells its frame to wait again. */
  private watch(): void {
    if (this.suspended > 0) return;
    let moved = false;
    for (const icon of this.icons) {
      if (icon.placed === null || icon.host.style.display === "none") continue;
      const rect = icon.host.getBoundingClientRect();
      const left = rect.left + window.scrollX;
      const top = rect.top + window.scrollY;
      if (
        Math.abs(left - icon.placed.left) > 1 ||
        Math.abs(top - icon.placed.top) > 1
      ) {
        moved = true;
        this.stun(icon);
      }
    }
    if (moved) this.place();
  }

  /**
   * After a move we did not make, the icon lets clicks through for a while
   * (a click right after it slid under the cursor reaches the page, not
   * us), and its frame restarts its visibility wait.
   */
  private stun(icon: Icon): void {
    icon.host.style.pointerEvents = "none";
    clearTimeout(icon.stunned);
    icon.stunned = setTimeout(() => {
      icon.host.style.pointerEvents = "";
    }, STUN_MS);
    icon.frame.contentWindow?.postMessage(
      { pwm: "moved" },
      new URL(icon.frame.src).origin,
    );
  }

  /** Keeps each icon at the right edge of its field, and the menu under it. */
  private place(): void {
    const inputs = this.inputs();
    if (this.suspended > 0) {
      for (const icon of this.icons) icon.host.style.display = "none";
      return;
    }
    // An open menu must not be covered by our other icons, which share its
    // z-index: the menu would count as covered and refuse every click.
    const menuOpen = this.icons.some((i) => i.open);
    for (const icon of this.icons) {
      const field = inputs[icon.index];
      const rect = field?.isConnected
        ? field.getBoundingClientRect()
        : undefined;
      if (
        !rect ||
        rect.width < ICON * 2 ||
        rect.height < 16 ||
        (menuOpen && !icon.open)
      ) {
        icon.host.style.display = "none";
        icon.placed = null;
        continue;
      }
      const x = window.scrollX;
      const y = window.scrollY;
      const style = icon.host.style;
      style.display = "block";
      if (icon.open) {
        const left = Math.max(
          0,
          Math.min(rect.left, window.innerWidth - MENU.width),
        );
        style.left = `${String(left + x)}px`;
        style.top = `${String(rect.bottom + y + 4)}px`;
        style.width = `${String(MENU.width)}px`;
        style.height = `${String(MENU.height)}px`;
      } else {
        style.left = `${String(rect.right + x - ICON - 4)}px`;
        style.top = `${String(rect.top + y + (rect.height - ICON) / 2)}px`;
        style.width = `${String(ICON)}px`;
        style.height = `${String(ICON)}px`;
      }
      const placed = icon.host.getBoundingClientRect();
      icon.placed = { left: placed.left + x, top: placed.top + y };
    }
  }
}

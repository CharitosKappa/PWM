/**
 * The save offer card (D36 §5): fixed at the top right, a closed shadow
 * root around the extension's own frame, like the inline icon. It never
 * moves the page's layout.
 */
const WIDTH = 340;
const HEIGHT = 168;

export class OfferCard {
  private host: HTMLElement | null = null;
  private suspended = 0;

  show(token: string): void {
    this.hide();
    const host = document.createElement("pwm-offer");
    host.style.cssText = `all: initial; position: fixed; top: 16px; right: 16px; width: ${String(WIDTH)}px; height: ${String(HEIGHT)}px; z-index: 2147483647; display: ${this.suspended > 0 ? "none" : "block"};`;
    const shadow = host.attachShadow({ mode: "closed" });
    const frame = document.createElement("iframe");
    frame.src = `${chrome.runtime.getURL("inline.html")}#offer:${encodeURIComponent(token)}`;
    frame.style.cssText =
      "all: initial; display: block; width: 100%; height: 100%; border: 0; color-scheme: normal;";
    shadow.append(frame);
    document.documentElement.append(host);
    this.host = host;
  }

  hide(): void {
    this.host?.remove();
    this.host = null;
  }

  /** Off the page while a fill is checked, like the icons; back afterwards. */
  hideWhile(): () => void {
    this.suspended += 1;
    if (this.host) this.host.style.display = "none";
    return () => {
      this.suspended -= 1;
      if (this.host && this.suspended === 0) this.host.style.display = "block";
    };
  }
}

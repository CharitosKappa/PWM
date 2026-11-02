/**
 * The inline icon frame (D35 §3), an extension page the content script puts
 * in a closed shadow root next to a sign-in field. It shows the icon, then
 * a menu of matching logins; every click must pass the visibility guard.
 * Its location hash holds the one-time token and the field's index.
 */
import type {
  InlineMenu,
  InlineRequest,
  Suggestion,
} from "../shared/autofill.ts";
import type { Reply } from "../shared/messages.ts";
import { ClickGuard } from "./guard.ts";

const [rawToken = "", rawIndex = ""] = location.hash.slice(1).split(":");
const token = decodeURIComponent(rawToken);
const index = Number(rawIndex);
const guard = new ClickGuard(() => performance.now());
const root = document.getElementById("root") as HTMLElement;

type Ask =
  | { kind: "menu" }
  | { kind: "fill"; vaultId: string; itemId: string }
  | { kind: "close" }
  | { kind: "hide" }
  | { kind: "unlock" };

async function ask(request: Ask): Promise<InlineMenu | null> {
  const message: InlineRequest = {
    target: "background",
    from: "inline",
    token,
    index,
    ...request,
  };
  const reply = await chrome.runtime.sendMessage<
    InlineRequest,
    Reply<InlineMenu | null> | undefined
  >(message);
  return reply?.ok ? reply.value : null;
}

const WARNINGS: Record<string, string> = {
  warn_insecure: "This page is not secure (http).",
  warn_regex: "Matched by a pattern. Check the address.",
  confirm: "Check the address before filling.",
};

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: { class?: string; text?: string; label?: string } = {},
  ...children: Node[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (props.class !== undefined) node.className = props.class;
  if (props.text !== undefined) node.textContent = props.text;
  if (props.label !== undefined) node.setAttribute("aria-label", props.label);
  node.append(...children);
  return node;
}

/** A button that acts only on clicks the guard accepts. */
function button(
  props: { class?: string; text?: string; label?: string },
  action: () => void,
  ...children: Node[]
): HTMLButtonElement {
  const node = el("button", props, ...children);
  node.type = "button";
  node.addEventListener("click", (event) => {
    if (guard.accepts(event)) action();
  });
  return node;
}

function iconGlyph(): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  // A key.
  path.setAttribute(
    "d",
    "M14 4a6 6 0 0 0-5.7 7.9L3 17.2V21h3.8l.9-.9v-1.8h1.8l.9-.9v-1.8h1.8l1-1A6 6 0 1 0 14 4zm2 3.5a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3z",
  );
  svg.append(path);
  return svg;
}

function render(...nodes: Node[]): void {
  root.replaceChildren(...nodes);
  guard.viewChanged();
}

function showIcon(): void {
  document.body.className = "icon";
  render(
    button(
      { class: "icon", label: "Fill with PWM" },
      () => void openMenu(),
      iconGlyph(),
    ),
  );
}

async function close(): Promise<void> {
  showIcon();
  await ask({ kind: "close" });
}

function item(s: Suggestion): HTMLElement {
  const warning = WARNINGS[s.decision];
  return el(
    "li",
    {},
    button(
      { class: "item" },
      () =>
        void ask({ kind: "fill", vaultId: s.vaultId, itemId: s.itemId }).then(
          showIcon,
        ),
      el("span", { class: "title", text: s.title || "Untitled" }),
      el("span", { class: "muted", text: s.username }),
      ...(warning === undefined
        ? []
        : [el("span", { class: "warning", text: warning })]),
    ),
  );
}

async function openMenu(): Promise<void> {
  document.body.className = "menu";
  render(el("p", { class: "muted", text: "Loading…" }));
  const menu = await ask({ kind: "menu" });
  if (!menu) {
    showIcon();
    return;
  }
  const footer = el(
    "footer",
    {},
    button(
      { class: "link", text: "Don't show here" },
      () => void ask({ kind: "hide" }),
    ),
    button({ class: "link", text: "Close" }, () => void close()),
  );
  if (menu.state === "locked") {
    render(
      el("p", { text: "PWM is locked." }),
      button(
        { class: "primary", text: "Unlock" },
        () => void ask({ kind: "unlock" }).then(showIcon),
      ),
      footer,
    );
    return;
  }
  const list =
    menu.items.length === 0
      ? el("p", { class: "muted", text: "No logins for this site." })
      : el("ul", {}, ...menu.items.map(item));
  render(list, footer);
}

// Visibility as the browser sees it, across every ancestor frame.
if ("isVisible" in IntersectionObserverEntry.prototype) {
  new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        guard.observed(
          (entry as IntersectionObserverEntry & { isVisible?: boolean })
            .isVisible,
        );
      }
    },
    { trackVisibility: true, delay: 100 } as IntersectionObserverInit,
  ).observe(document.documentElement);
}

window.addEventListener("blur", () => {
  if (document.body.className === "menu") void close();
});
window.addEventListener("keydown", (event) => {
  if (event.key === "Escape") void close();
});

showIcon();

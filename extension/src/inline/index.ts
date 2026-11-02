/**
 * The extension's frame inside pages, in a closed shadow root: the inline
 * icon next to a sign-in field (D35 §3: the icon, then a menu of matching
 * logins) or the save offer card (D36 §5). Every click must pass the
 * visibility guard. The location hash holds the one-time token and the
 * field's index (`token:index`), or `offer:token` for the card.
 */
import type {
  InlineMenu,
  InlineRequest,
  OfferView,
  Suggestion,
} from "../shared/autofill.ts";
import type { Reply } from "../shared/messages.ts";
import { ClickGuard } from "./guard.ts";

const [first = "", second = ""] = location.hash.slice(1).split(":");
const card = first === "offer";
const token = decodeURIComponent(card ? second : first);
const index = card ? 0 : Number(second);
const guard = new ClickGuard(() => performance.now());
const root = document.getElementById("root") as HTMLElement;

type Ask =
  | { kind: "menu" }
  | { kind: "fill"; vaultId: string; itemId: string }
  | { kind: "close" }
  | { kind: "hide" }
  | { kind: "unlock" }
  | { kind: "offer" }
  | { kind: "offerSave"; itemId: string | null }
  | { kind: "offerDismiss" }
  | { kind: "offerNever" };

async function ask(request: Ask): Promise<InlineMenu | null> {
  const reply = await send(request);
  return reply?.ok ? (reply.value as InlineMenu | null) : null;
}

async function send(
  request: Ask,
): Promise<Reply<InlineMenu | OfferView | null> | undefined> {
  const message: InlineRequest = {
    target: "background",
    from: "inline",
    token,
    index,
    ...request,
  };
  const reply = await chrome.runtime.sendMessage<
    InlineRequest,
    Reply<InlineMenu | OfferView | null> | undefined
  >(message);
  return reply;
}

const FAILURES: Record<string, string> = {
  field_hidden: "The field is hidden or covered, so nothing was filled.",
  no_form: "No sign-in form found here.",
  refused: "This login is not for this page.",
  page_changed: "The page changed. Try again.",
};

async function fill(s: Suggestion): Promise<void> {
  const reply = await send({
    kind: "fill",
    vaultId: s.vaultId,
    itemId: s.itemId,
  });
  if (reply?.ok) {
    showIcon();
    return;
  }
  const code = reply?.error ?? "internal";
  render(
    el("p", {
      class: "warning",
      text: FAILURES[code] ?? "Something went wrong. Try again.",
    }),
    button({ class: "link", text: "Close" }, () => void close()),
  );
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
      () => void fill(s),
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

// --- The save offer card (D36 §5) ---

async function offerAct(request: Ask): Promise<void> {
  const reply = await send(request);
  if (reply?.ok) return;
  render(
    el("p", {
      class: "warning",
      text: "Couldn't save. Open PWM and try again.",
    }),
  );
}

async function showOffer(): Promise<void> {
  document.body.className = "offer";
  const reply = await send({ kind: "offer" });
  const view = reply?.ok ? (reply.value as OfferView | null) : null;
  if (!view) {
    render();
    return;
  }
  const heading =
    view.action === "update"
      ? `Update the password for ${view.username || view.site}?`
      : `Save the login for ${view.site}?`;
  const parts: Node[] = [el("p", { class: "title", text: heading })];
  if (view.action === "save" && view.username !== "") {
    parts.push(el("p", { class: "muted", text: view.username }));
  }
  if (view.insecure) {
    parts.push(
      el("p", { class: "warning", text: "This page is not secure (http)." }),
    );
  }
  const later = button(
    { class: "link", text: "Not now" },
    () => void offerAct({ kind: "offerDismiss" }),
  );
  const never = button(
    { class: "link", text: "Never for this site" },
    () => void offerAct({ kind: "offerNever" }),
  );
  if (view.locked) {
    parts.push(
      button(
        { class: "primary", text: "Unlock to save" },
        () => void offerAct({ kind: "unlock" }),
      ),
    );
  } else if (view.action === "update" && view.candidates.length > 1) {
    // Several logins with that username: the user picks which.
    for (const c of view.candidates) {
      parts.push(
        button(
          { class: "item", text: `Update ${c.title || "Untitled"}` },
          () => void offerAct({ kind: "offerSave", itemId: c.itemId }),
        ),
      );
    }
  } else {
    const itemId =
      view.action === "update" ? (view.candidates[0]?.itemId ?? null) : null;
    parts.push(
      button(
        {
          class: "primary",
          text: view.action === "update" ? "Update" : "Save",
        },
        () => void offerAct({ kind: "offerSave", itemId }),
      ),
    );
  }
  parts.push(el("footer", {}, later, never));
  render(...parts);
}

window.addEventListener("blur", () => {
  if (document.body.className === "menu") void close();
});
window.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !card) void close();
});

if (card) void showOffer();
else showIcon();

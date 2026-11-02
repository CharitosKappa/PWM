/**
 * The passkey confirmation window (D37 §4): an extension page in a window
 * of its own, so the page that asked cannot cover or click it. It shows
 * the site and account, and asks to create, to sign in with a chosen
 * passkey, or to hand the request to the browser.
 */
import type { Reply } from "../shared/messages.ts";
import type {
  PasskeyPrompt,
  PasskeyWindowRequest,
} from "../shared/passkeys.ts";

const id = location.hash.slice(1);
const root = document.getElementById("root") as HTMLElement;

type Ask =
  | { kind: "prompt" }
  | { kind: "unlock"; password: string }
  | { kind: "create" }
  | { kind: "get"; vaultId: string; itemId: string; credentialId: string }
  | { kind: "browser" }
  | { kind: "cancel" };

async function send(
  request: Ask,
): Promise<Reply<PasskeyPrompt | null> | undefined> {
  const message: PasskeyWindowRequest = {
    target: "background",
    from: "passkeyWindow",
    id,
    ...request,
  };
  return chrome.runtime.sendMessage<
    PasskeyWindowRequest,
    Reply<PasskeyPrompt | null> | undefined
  >(message);
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: { class?: string; text?: string } = {},
  ...children: Node[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (props.class !== undefined) node.className = props.class;
  if (props.text !== undefined) node.textContent = props.text;
  node.append(...children);
  return node;
}

function button(
  text: string,
  className: string,
  action: () => void,
): HTMLButtonElement {
  const node = el("button", { class: className, text });
  node.type = "button";
  node.addEventListener("click", action);
  return node;
}

/** Sends a decision; the background closes the window when it is final. */
async function decide(request: Ask): Promise<void> {
  const reply = await send(request);
  if (reply && !reply.ok) {
    show({ state: "gone" }, "Something went wrong. Try again on the site.");
  } else {
    window.close();
  }
}

function footer(): HTMLElement {
  return el(
    "footer",
    { class: "row" },
    button(
      "Use another device or the browser",
      "link",
      () => void decide({ kind: "browser" }),
    ),
    button("Cancel", "", () => void decide({ kind: "cancel" })),
  );
}

function show(prompt: PasskeyPrompt, error = ""): void {
  const parts: Node[] = [];
  switch (prompt.state) {
    case "gone":
      parts.push(
        el("p", {
          text: error || "This request has ended. You can close this window.",
        }),
      );
      root.replaceChildren(...parts);
      return;
    case "locked": {
      parts.push(el("h1", { text: `Unlock PWM for ${prompt.site}` }));
      const input = el("input");
      input.type = "password";
      input.autocomplete = "current-password";
      input.setAttribute("aria-label", "Master password");
      const form = el("form", { class: "stack" }, input);
      const message = el("p", { class: "warning", text: error || " " });
      form.append(message, el("button", { class: "primary", text: "Unlock" }));
      form.addEventListener("submit", (event) => {
        event.preventDefault();
        void send({ kind: "unlock", password: input.value }).then((reply) => {
          input.value = "";
          if (reply?.ok && reply.value) show(reply.value);
          else show(prompt, "Wrong master password.");
        });
      });
      parts.push(form, footer());
      root.replaceChildren(...parts);
      input.focus();
      return;
    }
    case "create":
      parts.push(
        el("h1", { text: "Create a passkey?" }),
        el("p", {
          text: `For ${prompt.userName || "your account"} on ${prompt.rpId}.`,
        }),
        el("p", {
          class: "muted",
          text: "It is saved in your vault and syncs to your devices.",
        }),
        button(
          "Create passkey",
          "primary",
          () => void decide({ kind: "create" }),
        ),
        footer(),
      );
      root.replaceChildren(...parts);
      return;
    case "get":
      parts.push(el("h1", { text: `Sign in to ${prompt.site}` }));
      for (const c of prompt.candidates) {
        parts.push(
          button(
            `${c.userName || "Account"} · ${c.title}`,
            "item",
            () =>
              void decide({
                kind: "get",
                vaultId: c.vaultId,
                itemId: c.itemId,
                credentialId: c.credentialId,
              }),
          ),
        );
      }
      parts.push(footer());
      root.replaceChildren(...parts);
      return;
  }
}

void send({ kind: "prompt" }).then((reply) => {
  show(reply?.ok && reply.value ? reply.value : { state: "gone" });
});

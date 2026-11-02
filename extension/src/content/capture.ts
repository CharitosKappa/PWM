/**
 * Reading what the user typed when they submit (D36 §2). Values are read
 * only at that moment, from the fields the background named, sent once and
 * not kept. After a submit, the password field going away tells the
 * background the login worked.
 */
import type { Typed, Watch } from "../shared/autofill.ts";
import { looksVisible, origin } from "./dom.ts";

const DEBOUNCE_MS = 1000;
const GONE_POLL_MS = 500;
const GONE_WITHIN_MS = 10_000;

/** Reads the named fields now. Two new passwords must agree, or none is sent. */
export function readTyped(inputs: HTMLInputElement[], watch: Watch): Typed {
  const value = (i: number | null): string =>
    i === null ? "" : (inputs[i]?.value ?? "");
  const fresh = watch.newPassword.map((i) => value(i));
  const [first = "", second] = fresh;
  return {
    username: value(watch.username),
    password: value(watch.password),
    newPassword: second === undefined || second === first ? first : "",
  };
}

export class Capturer {
  private watch: Watch | null = null;
  private listening = false;
  private lastAt = 0;
  private goneTimer: ReturnType<typeof setInterval> | undefined;

  constructor(
    private readonly inputs: () => HTMLInputElement[],
    private readonly send: (message: object) => void,
  ) {}

  setWatch(watch: Watch | null): void {
    this.watch = watch;
    if (watch && !this.listening) this.listen();
  }

  private watched(): HTMLInputElement[] {
    const w = this.watch;
    if (!w) return [];
    const inputs = this.inputs();
    return [w.username, w.password, ...w.newPassword]
      .filter((i): i is number => i !== null)
      .map((i) => inputs[i])
      .filter((el): el is HTMLInputElement => el !== undefined);
  }

  private secret(): HTMLInputElement | undefined {
    const w = this.watch;
    if (!w) return undefined;
    const i = w.password ?? w.newPassword[0];
    return i === undefined ? undefined : this.inputs()[i];
  }

  private listen(): void {
    this.listening = true;
    document.addEventListener(
      "submit",
      (event) => {
        const form = event.target;
        if (
          form instanceof HTMLFormElement &&
          this.watched().some((el) => el.form === form)
        )
          this.submitted();
      },
      { capture: true },
    );
    document.addEventListener(
      "keydown",
      (event) => {
        if (event.key !== "Enter" || !event.isTrusted) return;
        const target = origin(event);
        if (
          target instanceof HTMLInputElement &&
          this.watched().includes(target)
        )
          this.submitted();
      },
      { capture: true },
    );
    // Single-page apps often sign in with a button and no form submit.
    document.addEventListener(
      "click",
      (event) => {
        if (!event.isTrusted) return;
        const button = origin(event)?.closest(
          'button, input[type="submit"], input[type="button"], [role="button"]',
        );
        if (!button) return;
        const secret = this.secret();
        if (!secret || secret.value === "") return;
        const form = secret.form;
        if (form === null || form.contains(button)) this.submitted();
      },
      { capture: true },
    );
  }

  private submitted(): void {
    const watch = this.watch;
    if (!watch) return;
    const now = Date.now();
    if (now - this.lastAt < DEBOUNCE_MS) return;
    const typed = readTyped(this.inputs(), watch);
    if (
      typed.username === "" &&
      typed.password === "" &&
      typed.newPassword === ""
    )
      return;
    this.lastAt = now;
    this.send({
      target: "background",
      from: "content",
      kind: "submitted",
      typed,
    });
    if (typed.password !== "" || typed.newPassword !== "") this.watchGone();
  }

  /** Tells the background once the password field is gone (D36 §3). */
  private watchGone(): void {
    clearInterval(this.goneTimer);
    const field = this.secret();
    if (!field) return;
    const started = Date.now();
    this.goneTimer = setInterval(() => {
      if (Date.now() - started > GONE_WITHIN_MS) {
        clearInterval(this.goneTimer);
        return;
      }
      if (!field.isConnected || !looksVisible(field)) {
        clearInterval(this.goneTimer);
        this.send({ target: "background", from: "content", kind: "formGone" });
      }
    }, GONE_POLL_MS);
  }
}

/**
 * Passkeys in the background (D37): checks each page call, asks the user
 * in an extension window (never page DOM), and has the core create or sign.
 * The origin is the browser's report of the calling frame.
 */
import type {
  PasskeyAnswer,
  PasskeyPrompt,
  PasskeyWindowRequest,
} from "../shared/passkeys.ts";
import type { Core } from "./core.ts";
import { errorCode } from "./core.ts";
import { ServiceError } from "./errors.ts";
import type { FrameSender, Platform } from "./platform.ts";

/** The window closes and the page gets NotAllowedError after this, at most. */
export const MAX_WAIT_MS = 10 * 60_000;
const DEFAULT_WAIT_MS = 5 * 60_000;
const MIN_WAIT_MS = 30_000;

export interface PasskeysHost {
  core(): Core;
  platform: Platform;
  /** "Use PWM for passkeys" is on and site access is granted. */
  enabled(): Promise<boolean>;
  /** A passkey was stored: save the cache and sync. */
  saved(): Promise<void>;
  unlock(password: string): Promise<void>;
  touch(): Promise<void>;
}

interface Pending {
  op: "create" | "get";
  request: string;
  origin: string;
  topOrigin: string | undefined;
  site: string;
  windowId: number | null;
  timer: ReturnType<typeof setTimeout>;
  resolve: (answer: PasskeyAnswer) => void;
}

interface Candidate {
  vault_id: string;
  item_id: string;
  credential_id: string;
  user_name: string;
  title: string;
}

function origin(url: string): string | null {
  try {
    const parsed = new URL(url);
    return parsed.origin === "null" ? null : parsed.origin;
  } catch {
    return null;
  }
}

/** The last two host labels: frames must be of the top page's site. */
function site(url: string): string {
  try {
    return new URL(url).hostname.split(".").slice(-2).join(".");
  } catch {
    return "";
  }
}

function waitFor(request: string): number {
  try {
    const timeout = Number(
      (JSON.parse(request) as { timeout?: unknown }).timeout,
    );
    if (Number.isFinite(timeout) && timeout > 0) {
      return Math.min(MAX_WAIT_MS, Math.max(MIN_WAIT_MS, timeout));
    }
  } catch {
    // Malformed: the core refuses it below.
  }
  return DEFAULT_WAIT_MS;
}

export class Passkeys {
  private pending = new Map<string, Pending>();

  constructor(private readonly host: PasskeysHost) {}

  private get core(): Core {
    return this.host.core();
  }

  /** A page called WebAuthn (D37 §1, §2). */
  async onCall(
    sender: FrameSender,
    op: "create" | "get",
    request: string,
  ): Promise<PasskeyAnswer> {
    if (!(await this.host.enabled())) return { fallback: true };
    const from = origin(sender.url);
    const top = origin(sender.tabUrl);
    if (from === null || top === null) return { fallback: true };
    // A frame of another site never gets ours.
    if (sender.frameId !== 0 && site(sender.url) !== site(sender.tabUrl)) {
      return { fallback: true };
    }
    const topOrigin = from === top ? undefined : top;
    const unlocked = this.core.isUnlocked();
    try {
      if (op === "get") {
        // Locked, or nothing for this site: the browser's turn, without asking.
        if (
          !unlocked ||
          this.candidates(request, from, topOrigin).length === 0
        ) {
          return { fallback: true };
        }
      } else if (unlocked) {
        const check = JSON.parse(
          this.core.passkeyCreateCheck(from, topOrigin, request),
        ) as { kind: string };
        if (check.kind !== "ready") return { fallback: true };
      }
    } catch (error) {
      // An RP id that is not the page's is refused; anything else is the browser's.
      return errorCode(error) === "invalid_rp_id"
        ? { error: "SecurityError" }
        : { fallback: true };
    }
    return this.ask(op, request, from, topOrigin);
  }

  private candidates(
    request: string,
    from: string,
    topOrigin: string | undefined,
  ): Candidate[] {
    return JSON.parse(
      this.core.passkeyCandidates(from, topOrigin, request),
    ) as Candidate[];
  }

  /** Opens the confirmation window and waits for the user (D37 §4). */
  private ask(
    op: "create" | "get",
    request: string,
    from: string,
    topOrigin: string | undefined,
  ): Promise<PasskeyAnswer> {
    const id = crypto.randomUUID();
    return new Promise<PasskeyAnswer>((resolve) => {
      const pending: Pending = {
        op,
        request,
        origin: from,
        topOrigin,
        site: new URL(from).hostname,
        windowId: null,
        timer: setTimeout(() => {
          void this.finish(id, { error: "NotAllowedError" });
        }, waitFor(request)),
        resolve,
      };
      this.pending.set(id, pending);
      void this.host.platform.pages
        .openWindow(`passkey.html#${id}`)
        .then((windowId) => {
          if (this.pending.has(id)) pending.windowId = windowId;
          else void this.host.platform.pages.closeWindow(windowId);
        });
    });
  }

  private async finish(id: string, answer: PasskeyAnswer): Promise<void> {
    const pending = this.pending.get(id);
    if (!pending) return;
    this.pending.delete(id);
    clearTimeout(pending.timer);
    pending.resolve(answer);
    if (pending.windowId !== null) {
      await this.host.platform.pages.closeWindow(pending.windowId);
    }
  }

  /** The user closed the window: a cancel. */
  async onWindowClosed(windowId: number): Promise<void> {
    for (const [id, pending] of this.pending) {
      if (pending.windowId === windowId) {
        await this.finish(id, { error: "NotAllowedError" });
      }
    }
  }

  private prompt(pending: Pending): PasskeyPrompt {
    if (!this.core.isUnlocked()) return { state: "locked", site: pending.site };
    if (pending.op === "create") {
      const check = JSON.parse(
        this.core.passkeyCreateCheck(
          pending.origin,
          pending.topOrigin,
          pending.request,
        ),
      ) as { kind: string; rp_id?: string; user_name?: string };
      return {
        state: "create",
        site: pending.site,
        rpId: check.rp_id ?? "",
        userName: check.user_name ?? "",
      };
    }
    const candidates = this.candidates(
      pending.request,
      pending.origin,
      pending.topOrigin,
    );
    return {
      state: "get",
      site: pending.site,
      rpId: candidates.length > 0 ? pending.site : "",
      candidates: candidates.map((c) => ({
        vaultId: c.vault_id,
        itemId: c.item_id,
        credentialId: c.credential_id,
        userName: c.user_name,
        title: c.title,
      })),
    };
  }

  /** A request from the confirmation window. */
  async onWindow(request: PasskeyWindowRequest): Promise<PasskeyPrompt | null> {
    const pending = this.pending.get(request.id);
    if (!pending) return { state: "gone" };
    switch (request.kind) {
      case "prompt": {
        const view = this.prompt(pending);
        // After unlocking, a create the core hands back goes to the browser.
        if (
          pending.op === "create" &&
          view.state === "create" &&
          view.rpId === ""
        ) {
          await this.finish(request.id, { fallback: true });
          return { state: "gone" };
        }
        return view;
      }
      case "unlock":
        await this.host.unlock(request.password);
        return this.onWindow({ ...request, kind: "prompt" });
      case "browser":
        await this.finish(request.id, { fallback: true });
        return null;
      case "cancel":
        await this.finish(request.id, { error: "NotAllowedError" });
        return null;
      case "create": {
        if (pending.op !== "create") throw new ServiceError("bad_request");
        const vaultId = this.core.vaultIds()[0];
        if (vaultId === undefined) throw new ServiceError("no_vault");
        const check = JSON.parse(
          this.core.passkeyCreateCheck(
            pending.origin,
            pending.topOrigin,
            pending.request,
          ),
        ) as { kind: string };
        if (check.kind !== "ready") {
          await this.finish(request.id, { fallback: true });
          return null;
        }
        const result = this.core.passkeyCreate(
          vaultId,
          pending.origin,
          pending.topOrigin,
          pending.request,
          this.host.platform.now(),
        );
        await this.host.saved();
        await this.host.touch();
        await this.finish(request.id, { result });
        return null;
      }
      case "get": {
        if (pending.op !== "get") throw new ServiceError("bad_request");
        const result = this.core.passkeyGet(
          pending.origin,
          pending.topOrigin,
          pending.request,
          request.vaultId,
          request.itemId,
          request.credentialId,
        );
        await this.host.touch();
        await this.finish(request.id, { result });
        return null;
      }
    }
  }

  /** "Use PWM for passkeys" (D37 §1): on only with site access. */
  async setEnabled(on: boolean): Promise<boolean> {
    const allowed = on && (await this.host.platform.pages.hasSiteAccess());
    await this.host.platform.disk.set("passkeys", allowed ? "on" : "off");
    await this.host.platform.pages.setPasskeyScripts(allowed);
    return allowed;
  }
}

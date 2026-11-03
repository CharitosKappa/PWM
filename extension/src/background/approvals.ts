/**
 * Approving a new device from a signed-in one (D46). Asking: while signed
 * out, the popup asks a signed-in device and polls; the code is shown to
 * compare, and the Secret Key that arrives stays here until the master
 * password is typed. Approving: the manage page lists requests, accepts
 * one, and sends this device's Secret Key once the user saw the codes
 * match. The badge counts requests, read at sync.
 */
import type { ApprovalView, AskView } from "../shared/messages.ts";
import type {
  Core,
  CoreApprovalAsk,
  CoreAskStep,
  CoreFunctions,
  CoreStep,
} from "./core.ts";
import { errorCode } from "./core.ts";
import { ServiceError } from "./errors.ts";
import { sendOnce } from "./http.ts";
import type { Platform } from "./platform.ts";

interface ApprovalDeps {
  core: () => Core;
  fns: CoreFunctions;
  platform: Platform;
  server: () => string;
  /** A server conversation of the signed-in core, queued with the others. */
  run: (start: () => CoreStep) => Promise<CoreStep>;
  /** The end of a run that is not this request's outcome (revoked, offline). */
  ended: (step: CoreStep) => Promise<void>;
  secretKey: () => Promise<string | undefined>;
  signIn: (email: string, password: string, secretKey: string) => Promise<void>;
}

interface ApprovalJson {
  id: string;
  platform: string;
  created_at: number;
  state: string;
  code: string | null;
}

export class Approvals {
  private asking: CoreApprovalAsk | null = null;
  private email = "";
  private code: string | null = null;
  /** The Secret Key from the signed-in device, until the password is typed. */
  private approvedKey: string | null = null;
  /** Requests from new devices, for the status and the badge. */
  pending = 0;

  constructor(private readonly deps: ApprovalDeps) {}

  // --- Asking (the new device) ---

  async ask(email: string): Promise<AskView> {
    this.cancel();
    let asking: CoreApprovalAsk;
    try {
      asking = this.deps.fns.newApprovalAsk(email.trim());
    } catch (error) {
      throw new ServiceError(errorCode(error));
    }
    this.asking = asking;
    this.email = email.trim();
    const first = asking.firstRequest();
    if (!first) throw new ServiceError("internal");
    return this.advance(await this.exchange(asking, first));
  }

  /** Called by the popup every 2 seconds while it shows the request. */
  async poll(): Promise<AskView> {
    if (this.approvedKey !== null) return { state: "approved", code: null };
    const asking = this.asking;
    if (!asking) return { state: "ended", code: null };
    return this.advance(
      await this.exchange(
        asking,
        this.call(() => asking.poll()),
      ),
    );
  }

  /** Gives up, on the server too (without waiting). */
  cancel(): void {
    const asking = this.asking;
    if (asking) {
      const request = asking.cancel();
      if (request)
        void sendOnce(
          request,
          this.deps.server(),
          this.deps.platform.fetch,
        ).catch(() => undefined);
    }
    this.clearAsk();
  }

  /** Signs in with the Secret Key that arrived and the master password. */
  async signInApproved(password: string): Promise<void> {
    const secretKey = this.approvedKey;
    if (secretKey === null) throw new ServiceError("approval_ended");
    await this.deps.signIn(this.email, password, secretKey);
    this.clearAsk();
  }

  private clearAsk(): void {
    this.asking = null;
    this.code = null;
    this.approvedKey = null;
  }

  private async exchange(
    asking: CoreApprovalAsk,
    request: CoreStep,
  ): Promise<CoreAskStep> {
    let answer;
    try {
      answer = await sendOnce(
        request,
        this.deps.server(),
        this.deps.platform.fetch,
      );
    } catch {
      throw new ServiceError("network");
    }
    return this.call(() =>
      asking.receive(answer.status, answer.headers, answer.body),
    );
  }

  private async advance(step: CoreAskStep): Promise<AskView> {
    switch (step.kind) {
      case "send": {
        const request = step.request();
        const asking = this.asking;
        if (!request || !asking) throw new ServiceError("internal");
        return this.advance(await this.exchange(asking, request));
      }
      case "code":
        this.code = step.code ?? null;
        return { state: "code", code: this.code };
      case "approved":
        this.approvedKey = step.secretKey ?? null;
        this.code = null;
        return { state: "approved", code: null };
      case "wait":
        return {
          state: this.code === null ? "waiting" : "code",
          code: this.code,
        };
      default:
        this.clearAsk();
        return { state: "ended", code: null };
    }
  }

  // --- Approving (the signed-in device) ---

  async list(): Promise<ApprovalView[]> {
    const step = await this.conversation(
      () => this.deps.core().startListApprovals(),
      "approvals",
    );
    const list = (JSON.parse(step.approvals ?? "[]") as ApprovalJson[]).map(
      (a) => ({
        id: a.id,
        platform: a.platform,
        createdAt: a.created_at,
        state: a.state,
        code: a.code,
      }),
    );
    await this.setPending(list.length);
    return list;
  }

  async accept(id: string): Promise<ApprovalView[]> {
    await this.conversation(
      () => this.deps.core().startAcceptApproval(id),
      "approval_updated",
    );
    return this.list();
  }

  /** After the user saw the same code on both devices. */
  async complete(id: string): Promise<ApprovalView[]> {
    const secretKey = await this.deps.secretKey();
    if (secretKey === undefined) throw new ServiceError("signed_out");
    await this.conversation(
      () => this.deps.core().startCompleteApproval(id, secretKey),
      "approval_updated",
    );
    return this.list();
  }

  async deny(id: string): Promise<ApprovalView[]> {
    await this.conversation(
      () => this.deps.core().startDenyApproval(id),
      "approval_updated",
    );
    return this.list();
  }

  /** After a sync: how many new devices are asking, on the badge. */
  async refresh(): Promise<void> {
    try {
      await this.list();
    } catch {
      // Shown at the next sync; the vault works without it.
    }
  }

  private async setPending(count: number): Promise<void> {
    this.pending = count;
    await this.deps.platform.pages.setGlobalBadge(
      count > 0 ? String(count) : "",
    );
  }

  private async conversation(
    start: () => CoreStep,
    outcome: string,
  ): Promise<CoreStep> {
    const step = await this.deps.run(start);
    if (step.kind === "done" && step.outcome === outcome) return step;
    await this.deps.ended(step);
    if (step.kind === "wait") throw new ServiceError("network");
    throw new ServiceError(step.outcome ?? "internal");
  }

  private call<T>(fn: () => T): T {
    try {
      return fn();
    } catch (error) {
      throw new ServiceError(errorCode(error));
    }
  }
}

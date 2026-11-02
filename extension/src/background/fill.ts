/**
 * Filling on user action (D35): the popup's Fill button, the shortcut and
 * the inline icon. Content scripts only describe fields and set the values
 * the background sends to one chosen document; they never see the vault.
 */
import type {
  FieldInfo,
  FillReply,
  FrameReply,
  InlineMenu,
  InlineRequest,
  Suggestion,
} from "../shared/autofill.ts";
import type { Core } from "./core.ts";
import { ServiceError } from "./errors.ts";
import { iconFields, planFill } from "./plan.ts";
import type { FieldClass, Plan } from "./plan.ts";
import type { FrameRef, FrameSender, Platform } from "./platform.ts";

/** How long a scan waits for every injected frame to report. */
const SCAN_TIMEOUT_MS = 1500;
const MAX_TOKENS = 500;

/** What the fill needs from the background, without the rest of it. */
export interface AutofillHost {
  core(): Core;
  platform: Platform;
  /** Copies with the 30-second clear (D34). */
  copy(text: string): Promise<void>;
  touch(): Promise<void>;
  login(
    vaultId: string,
    itemId: string,
  ): { username: string; password: string; hasCode: boolean } | undefined;
}

interface FrameState {
  ref: FrameRef;
  url: string;
  tabUrl: string;
  fields: FieldInfo[];
  classes: FieldClass[];
  seenAt: number;
}

interface MatchJson {
  vault_id: string;
  item_id: string;
  title: string;
  username: string;
  decision: string;
}

export interface FillResult {
  copiedCode: boolean;
}

function isWebPage(url: string): boolean {
  return url.startsWith("https://") || url.startsWith("http://");
}

function hostname(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return "";
  }
}

function parseClasses(json: string, count: number): FieldClass[] {
  const value: unknown = JSON.parse(json);
  if (!Array.isArray(value) || value.length !== count) return [];
  return value.map((c: unknown) => {
    const o = (
      typeof c === "object" && c !== null ? c : {}
    ) as Partial<FieldClass>;
    return {
      class: typeof o.class === "string" ? o.class : "ignore",
      confidence: typeof o.confidence === "number" ? o.confidence : 0,
    };
  });
}

function parseMatches(json: string): MatchJson[] {
  const value: unknown = JSON.parse(json);
  return Array.isArray(value) ? (value as MatchJson[]) : [];
}

const refused = (decision: string): boolean => decision.startsWith("refuse");

export class Autofill {
  /** The last report of each document, per tab and frame. */
  private frames = new Map<number, Map<number, FrameState>>();
  private waiters: (() => void)[] = [];
  /** Inline icon tokens (D35 §3): which document each icon frame belongs to. */
  private tokens = new Map<string, FrameState["ref"]>();

  constructor(private readonly host: AutofillHost) {}

  private get core(): Core {
    return this.host.core();
  }

  private get platform(): Platform {
    return this.host.platform;
  }

  // --- Frame reports ---

  /** A content script described its frame. Answers where to show the inline icon. */
  async onFrameReport(
    sender: FrameSender,
    fields: FieldInfo[],
  ): Promise<FrameReply> {
    let classes: FieldClass[];
    try {
      classes = parseClasses(
        this.core.classifyFields(JSON.stringify(fields), sender.url),
        fields.length,
      );
    } catch {
      // Unreadable fields: nothing is classified, so nothing is filled.
      classes = [];
    }
    const state: FrameState = {
      ref: {
        tabId: sender.tabId,
        frameId: sender.frameId,
        documentId: sender.documentId,
      },
      url: sender.url,
      tabUrl: sender.tabUrl,
      fields,
      classes,
      seenAt: this.platform.now(),
    };
    let tab = this.frames.get(sender.tabId);
    if (!tab) {
      tab = new Map();
      this.frames.set(sender.tabId, tab);
    }
    tab.set(sender.frameId, state);
    for (const wake of this.waiters.splice(0)) wake();

    const none: FrameReply = { token: null, icons: [] };
    if (!isWebPage(sender.url) || !(await this.inlineIcon())) return none;
    if ((await this.hiddenSites()).includes(hostname(sender.tabUrl)))
      return none;
    const icons = iconFields(fields, classes);
    if (icons.length === 0) return none;
    const token = crypto.randomUUID();
    this.tokens.set(token, state.ref);
    if (this.tokens.size > MAX_TOKENS) {
      const oldest = this.tokens.keys().next().value;
      if (oldest !== undefined) this.tokens.delete(oldest);
    }
    return { token, icons };
  }

  private frameState(ref: FrameRef): FrameState | undefined {
    const state = this.frames.get(ref.tabId)?.get(ref.frameId);
    return state?.ref.documentId === ref.documentId ? state : undefined;
  }

  /** Injects the content script and waits until every reached frame reported. */
  private async scan(tabId: number): Promise<FrameState[]> {
    const since = this.platform.now();
    const injected = await this.platform.pages.inject(tabId);
    const deadline = Date.now() + SCAN_TIMEOUT_MS;
    const fresh = (): FrameState[] =>
      injected
        .map((f) => this.frameState({ tabId, ...f }))
        .filter((s): s is FrameState => s !== undefined && s.seenAt >= since);
    while (fresh().length < injected.length && Date.now() < deadline) {
      await new Promise<void>((resolve) => {
        this.waiters.push(resolve);
        setTimeout(resolve, Math.max(0, deadline - Date.now()));
      });
    }
    // The top frame first, then the others in the browser's order.
    return fresh().sort((a, b) => a.ref.frameId - b.ref.frameId);
  }

  onTabRemoved(tabId: number): void {
    this.frames.delete(tabId);
    for (const [token, ref] of this.tokens) {
      if (ref.tabId === tabId) this.tokens.delete(token);
    }
    try {
      this.core.autofillForgetTab(String(tabId));
    } catch {
      // Nothing recorded for it.
    }
  }

  // --- Matches ---

  private matches(
    tabId: number,
    tabUrl: string,
    frame?: FrameState,
  ): Suggestion[] {
    const frameUrl = frame && frame.ref.frameId !== 0 ? frame.url : undefined;
    const next = this.core.autofillNextStep(
      String(tabId),
      tabUrl,
      this.platform.now(),
    );
    const list = parseMatches(this.core.autofillMatches(tabUrl, frameUrl)).map(
      (m): Suggestion => ({
        vaultId: m.vault_id,
        itemId: m.item_id,
        title: m.title,
        username: m.username,
        decision: m.decision,
        next: next?.[0] === m.item_id,
      }),
    );
    // The login in progress on this tab comes first (D33 multi-step).
    return [...list.filter((s) => s.next), ...list.filter((s) => !s.next)];
  }

  /** The logins for the tab the popup was opened on. */
  async suggestions(): Promise<{ site: string; items: Suggestion[] }> {
    const tab = await this.platform.pages.activeTab();
    if (!tab || !isWebPage(tab.url)) return { site: "", items: [] };
    return {
      site: hostname(tab.url),
      items: this.matches(tab.id, tab.url).filter((s) => !refused(s.decision)),
    };
  }

  // --- Filling ---

  /** The popup's Fill button. `confirmed`: the user saw the warning or confirmation. */
  async fillActiveTab(
    vaultId: string,
    itemId: string,
    confirmed: boolean,
  ): Promise<FillResult> {
    const tab = await this.platform.pages.activeTab();
    if (!tab || !isWebPage(tab.url)) throw new ServiceError("no_page");
    return this.fillTab(tab.id, vaultId, itemId, confirmed);
  }

  /** Fills a login into the best form of a tab (also the browser tests' entry). */
  async fillTab(
    tabId: number,
    vaultId: string,
    itemId: string,
    confirmed: boolean,
  ): Promise<FillResult> {
    const login = this.host.login(vaultId, itemId);
    if (!login) throw new ServiceError("unknown_item");
    const frames = await this.scan(tabId);
    let refusedHere = false;
    for (const frame of frames) {
      const match = this.matches(tabId, frame.tabUrl, frame).find(
        (s) => s.itemId === itemId && s.vaultId === vaultId,
      );
      if (!match) continue;
      if (refused(match.decision)) {
        refusedHere = true;
        continue;
      }
      const plan = this.plan(frame, login, tabId);
      if (!plan) continue;
      if (match.decision !== "fill" && !confirmed) {
        throw new ServiceError("needs_confirm");
      }
      return this.fillFrame(frame, plan, vaultId, itemId, login, true);
    }
    throw new ServiceError(refusedHere ? "refused" : "no_form");
  }

  private plan(
    frame: FrameState,
    login: { username: string; password: string; hasCode: boolean },
    tabId: number,
  ): Plan | null {
    const next = this.core.autofillNextStep(
      String(tabId),
      frame.tabUrl,
      this.platform.now(),
    );
    return planFill(
      frame.fields,
      frame.classes,
      {
        username: login.username !== "",
        password: login.password !== "",
        code: login.hasCode,
      },
      next?.[1],
    );
  }

  private async fillFrame(
    frame: FrameState,
    plan: Plan,
    vaultId: string,
    itemId: string,
    login: { username: string; password: string },
    scroll: boolean,
  ): Promise<FillResult> {
    const now = this.platform.now();
    let code = "";
    if (plan.code !== undefined || plan.copyCode) {
      code = this.core.currentCode(vaultId, itemId, now)?.[0] ?? "";
    }
    const fills = [
      plan.username !== undefined
        ? { index: plan.username, value: login.username }
        : null,
      plan.password !== undefined
        ? { index: plan.password, value: login.password }
        : null,
      plan.code !== undefined && code !== ""
        ? { index: plan.code, value: code }
        : null,
    ].filter((f): f is { index: number; value: string } => f !== null);

    if (fills.length > 0) {
      // Only to this document: a frame that navigated meanwhile gets nothing.
      const reply = (await this.platform.pages.send(frame.ref, {
        target: "content",
        kind: "fill",
        fills,
        scroll,
      })) as FillReply | undefined;
      if (reply === undefined) throw new ServiceError("page_changed");
      if (!reply.ok) throw new ServiceError("field_hidden");
    }
    let copiedCode = false;
    if (plan.copyCode && code !== "") {
      await this.host.copy(code);
      copiedCode = true;
    }
    if (plan.step !== null) {
      this.core.autofillRecordStep(
        String(frame.ref.tabId),
        frame.tabUrl,
        itemId,
        plan.step,
        now,
      );
    }
    await this.host.touch();
    return { copiedCode };
  }

  // --- Shortcut ---

  /** The fill shortcut (D35 §2): one sure match fills, anything else opens the popup. */
  async onCommand(tab: { id: number; url: string } | undefined): Promise<void> {
    if (!tab || !isWebPage(tab.url) || !this.core.isUnlocked()) {
      await this.platform.pages.openPopup();
      return;
    }
    const items = this.matches(tab.id, tab.url).filter(
      (s) => !refused(s.decision),
    );
    const only = items.length === 1 ? items[0] : undefined;
    if (only?.decision !== "fill") {
      await this.platform.pages.openPopup();
      return;
    }
    try {
      await this.fillTab(tab.id, only.vaultId, only.itemId, false);
    } catch {
      await this.platform.pages.openPopup();
    }
  }

  // --- Inline icon ---

  /** On when the user turned it on and site access is still granted. */
  async inlineIcon(): Promise<boolean> {
    return (
      (await this.platform.disk.get("inlineIcon")) === "on" &&
      (await this.platform.pages.hasSiteAccess())
    );
  }

  /**
   * Turns the inline icon on (after the popup got site access) or off.
   * Site access can stay after turning it off (a self-hosted server may
   * need it), so the choice is stored too.
   */
  async setInlineIcon(on: boolean): Promise<boolean> {
    const allowed = on && (await this.platform.pages.hasSiteAccess());
    await this.platform.disk.set("inlineIcon", allowed ? "on" : "off");
    await this.platform.pages.setInlineScripts(allowed);
    if (!allowed) {
      for (const [token, ref] of this.tokens) {
        this.tokens.delete(token);
        await this.platform.pages.send(ref, {
          target: "content",
          kind: "removeIcons",
        });
      }
    }
    return allowed;
  }

  /** Site access was taken back in the browser: stop the content script. */
  async dropInlineIfNoAccess(): Promise<void> {
    if (!(await this.platform.pages.hasSiteAccess())) {
      await this.platform.pages.setInlineScripts(false);
    }
  }

  private async hiddenSites(): Promise<string[]> {
    const stored = await this.platform.disk.get("inlineHidden");
    if (stored === undefined) return [];
    try {
      const value: unknown = JSON.parse(stored);
      return Array.isArray(value)
        ? value.filter((v): v is string => typeof v === "string")
        : [];
    } catch {
      return [];
    }
  }

  /** A request from an inline icon frame inside tab `tabId`. */
  async onInline(
    request: InlineRequest,
    tabId: number,
  ): Promise<InlineMenu | null> {
    const ref = this.tokens.get(request.token);
    if (ref?.tabId !== tabId) throw new ServiceError("bad_token");
    const frame = this.frameState(ref);
    if (!frame) throw new ServiceError("page_changed");
    const menu = (open: boolean) =>
      this.platform.pages.send(ref, {
        target: "content",
        kind: "menu",
        index: request.index,
        open,
      });

    switch (request.kind) {
      case "menu": {
        await menu(true);
        if (!this.core.isUnlocked()) return { state: "locked" };
        await this.host.touch();
        return {
          state: "unlocked",
          items: this.matches(tabId, frame.tabUrl, frame).filter(
            (s) => !refused(s.decision),
          ),
        };
      }
      case "close":
        await menu(false);
        return null;
      case "unlock":
        await menu(false);
        await this.platform.pages.openPopup();
        return null;
      case "hide": {
        const sites = await this.hiddenSites();
        const site = hostname(frame.tabUrl);
        if (site !== "" && !sites.includes(site)) {
          await this.platform.disk.set(
            "inlineHidden",
            JSON.stringify([...sites, site]),
          );
        }
        await this.platform.pages.send(ref, {
          target: "content",
          kind: "removeIcons",
        });
        return null;
      }
      case "fill": {
        const login = this.host.login(request.vaultId, request.itemId);
        if (!login) throw new ServiceError("unknown_item");
        const match = this.matches(tabId, frame.tabUrl, frame).find(
          (s) => s.itemId === request.itemId && s.vaultId === request.vaultId,
        );
        // Choosing in the menu, which shows any warning, is the confirmation.
        if (!match || refused(match.decision))
          throw new ServiceError("refused");
        const plan = this.plan(frame, login, tabId);
        if (!plan) throw new ServiceError("no_form");
        await menu(false);
        await this.fillFrame(
          frame,
          plan,
          request.vaultId,
          request.itemId,
          login,
          false,
        );
        return null;
      }
    }
  }
}

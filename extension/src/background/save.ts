/**
 * Save and update offers after a sign-in (D36). Captured values live only
 * here, in memory, for at most 5 minutes; the core decides what to offer
 * and writes the login. The popup and the card see the site and username,
 * never the password.
 */
import type {
  InlineRequest,
  OfferCandidate,
  OfferView,
  Typed,
} from "../shared/autofill.ts";
import { isTyped } from "../shared/autofill.ts";
import type { Core } from "./core.ts";
import { ServiceError } from "./errors.ts";
import type { Autofill } from "./fill.ts";
import type { FrameRef, FrameSender, Platform } from "./platform.ts";

/** A capture is offered only if the login worked within this time (D36 §3). */
export const WORKED_WITHIN_MS = 10_000;
/** Captures and offers are forgotten after this (D36 §2). */
export const CAPTURE_TTL_MS = 5 * 60_000;

export interface SaverHost {
  core(): Core;
  platform: Platform;
  autofill: Autofill;
  /** "Fill and save on sign-in pages" is on (D36 §1). */
  enabled(): Promise<boolean>;
  /** A login was written: save the cache and sync. */
  saved(): Promise<void>;
  touch(): Promise<void>;
}

interface Capture {
  ref: FrameRef;
  tabUrl: string;
  /** The frame's URL when the form was in a frame of the same site. */
  frameUrl: string | undefined;
  typed: Typed;
  at: number;
  /** The top document when the user submitted; another one means it navigated. */
  topDocument: string | undefined;
  worked: boolean;
  /** Set once offered: the card's token. */
  token: string | null;
}

interface CoreOffer {
  kind: "nothing" | "save" | "update";
  candidates: {
    vault_id: string;
    item_id: string;
    title: string;
    username: string;
  }[];
}

function hostname(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return "";
  }
}

function title(url: string): string {
  const host = hostname(url);
  return host.startsWith("www.") ? host.slice(4) : host;
}

/** The last two labels: enough to keep a hint on the same site's pages. */
function siteKey(url: string): string {
  return hostname(url).split(".").slice(-2).join(".");
}

export class Saver {
  private captures = new Map<number, Capture>();
  /** Usernames typed on the first page of a multi-step login, per tab. */
  private hints = new Map<
    number,
    { site: string; username: string; at: number }
  >();

  constructor(private readonly host: SaverHost) {}

  private get core(): Core {
    return this.host.core();
  }

  private get platform(): Platform {
    return this.host.platform;
  }

  private now(): number {
    return this.platform.now();
  }

  private alive(capture: Capture | undefined): capture is Capture {
    return capture !== undefined && this.now() - capture.at <= CAPTURE_TTL_MS;
  }

  // --- Capturing ---

  /** The user submitted a form (D36 §2). */
  async onSubmitted(sender: FrameSender, typed: Typed): Promise<void> {
    if (!(await this.host.enabled())) return;
    await this.capture(sender, typed, false);
  }

  private async capture(
    sender: FrameSender,
    typed: Typed,
    worked: boolean,
  ): Promise<Capture | undefined> {
    const now = this.now();
    if (typed.password === "" && typed.newPassword === "") {
      // The first page of a multi-step login: keep the username for the next.
      if (typed.username.trim() !== "") {
        this.hints.set(sender.tabId, {
          site: siteKey(sender.tabUrl),
          username: typed.username,
          at: now,
        });
      }
      return undefined;
    }
    let username = typed.username;
    const hint = this.hints.get(sender.tabId);
    if (
      username.trim() === "" &&
      hint &&
      hint.site === siteKey(sender.tabUrl) &&
      now - hint.at <= CAPTURE_TTL_MS
    ) {
      username = hint.username;
    }
    await this.drop(sender.tabId);
    const capture: Capture = {
      ref: {
        tabId: sender.tabId,
        frameId: sender.frameId,
        documentId: sender.documentId,
      },
      tabUrl: sender.tabUrl,
      frameUrl: sender.frameId === 0 ? undefined : sender.url,
      typed: { ...typed, username },
      at: now,
      // The document that submitted, when the form is in the top frame.
      topDocument:
        sender.frameId === 0
          ? sender.documentId
          : this.host.autofill.topFrame(sender.tabId)?.documentId,
      worked,
      token: null,
    };
    this.captures.set(sender.tabId, capture);
    // The next page may have reported before this message arrived.
    const top = this.host.autofill.topFrame(sender.tabId);
    if (
      !worked &&
      top !== undefined &&
      top.documentId !== capture.topDocument &&
      siteKey(this.host.autofill.topUrl(sender.tabId) ?? "") ===
        siteKey(sender.tabUrl)
    ) {
      capture.worked = true;
    }
    if (capture.worked) await this.offer(capture);
    return capture;
  }

  /** The password field went away after the submit: the login worked. */
  async onFormGone(sender: FrameSender): Promise<void> {
    const capture = this.captures.get(sender.tabId);
    if (!this.alive(capture) || capture.worked) return;
    if (capture.ref.documentId !== sender.documentId) return;
    if (this.now() - capture.at > WORKED_WITHIN_MS) return;
    capture.worked = true;
    await this.offer(capture);
  }

  /**
   * A top frame reported. A new document of the same site soon after a
   * submit means the login worked; a pending offer shows its card there.
   */
  async onTopFrame(sender: FrameSender): Promise<void> {
    const capture = this.captures.get(sender.tabId);
    if (!this.alive(capture)) return;
    if (capture.worked) return;
    if (sender.documentId === capture.topDocument) return;
    if (this.now() - capture.at > WORKED_WITHIN_MS) return;
    if (siteKey(sender.url) !== siteKey(capture.tabUrl)) return;
    capture.worked = true;
    await this.offer(capture);
  }

  // --- Offering ---

  private async never(): Promise<string[]> {
    const stored = await this.platform.disk.get("saveNever");
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

  /** What the core says to offer; null when nothing (or locked: decided later). */
  private decide(capture: Capture): CoreOffer | null {
    if (!this.core.isUnlocked()) return null;
    const { username, password, newPassword } = capture.typed;
    const offer = JSON.parse(
      this.core.saveOffer(
        capture.tabUrl,
        capture.frameUrl,
        username,
        password,
        newPassword === "" ? undefined : newPassword,
      ),
    ) as CoreOffer;
    return offer.kind === "nothing" ? null : offer;
  }

  private async offer(capture: Capture): Promise<void> {
    if ((await this.never()).includes(hostname(capture.tabUrl))) {
      await this.drop(capture.ref.tabId);
      return;
    }
    if (this.core.isUnlocked() && this.decide(capture) === null) {
      await this.drop(capture.ref.tabId);
      return;
    }
    capture.token = crypto.randomUUID();
    await this.platform.pages.setBadge(capture.ref.tabId, "1");
    const top = this.host.autofill.topFrame(capture.ref.tabId);
    if (top) {
      await this.platform.pages.send(top, {
        target: "content",
        kind: "showOffer",
        token: capture.token,
      });
    }
  }

  private async drop(tabId: number): Promise<void> {
    const capture = this.captures.get(tabId);
    if (!capture) return;
    this.captures.delete(tabId);
    if (capture.token === null) return;
    await this.platform.pages.setBadge(tabId, "");
    const top = this.host.autofill.topFrame(tabId);
    if (top) {
      await this.platform.pages.send(top, {
        target: "content",
        kind: "hideOffer",
      });
    }
  }

  private offered(tabId: number): Capture | undefined {
    const capture = this.captures.get(tabId);
    if (!this.alive(capture) || capture.token === null) return undefined;
    return capture;
  }

  /** The offer as the popup and the card show it; the password stays here. */
  private view(capture: Capture): OfferView | null {
    const locked = !this.core.isUnlocked();
    const decided = locked ? null : this.decide(capture);
    if (!locked && decided === null) return null;
    const candidates: OfferCandidate[] = (decided?.candidates ?? []).map(
      (c) => ({
        vaultId: c.vault_id,
        itemId: c.item_id,
        title: c.title,
        username: c.username,
      }),
    );
    return {
      site: hostname(capture.tabUrl),
      action: decided?.kind === "update" ? "update" : "save",
      username: capture.typed.username.trim(),
      title: title(capture.frameUrl ?? capture.tabUrl),
      insecure: capture.tabUrl.startsWith("http:"),
      candidates,
      locked,
    };
  }

  /** The pending offer for the popup's tab. */
  async popupOffer(): Promise<OfferView | null> {
    const tab = await this.platform.pages.activeTab();
    if (!tab) return null;
    const capture = this.offered(tab.id);
    if (!capture) return null;
    const view = this.view(capture);
    if (!view) await this.drop(tab.id);
    return view;
  }

  /**
   * Saves (itemId null) or updates (itemId one of the candidates). The
   * popup may change the title and username of a new login.
   */
  private async accept(
    capture: Capture,
    itemId: string | null,
    edits: { title?: string; username?: string },
  ): Promise<void> {
    const decided = this.decide(capture);
    if (!decided)
      throw new ServiceError(
        this.core.isUnlocked() ? "nothing_to_save" : "locked",
      );
    const password = capture.typed.newPassword || capture.typed.password;
    const now = this.now();
    if (itemId === null) {
      const vaultId = this.core.vaultIds()[0];
      if (vaultId === undefined) throw new ServiceError("no_vault");
      this.core.saveLogin(
        vaultId,
        capture.frameUrl ?? capture.tabUrl,
        edits.title ?? "",
        edits.username ?? capture.typed.username,
        password,
        now,
      );
    } else {
      const candidate = decided.candidates.find((c) => c.item_id === itemId);
      if (!candidate) throw new ServiceError("unknown_item");
      this.core.updatePassword(candidate.vault_id, itemId, password, now);
    }
    await this.drop(capture.ref.tabId);
    await this.host.saved();
    await this.host.touch();
  }

  async acceptFromPopup(
    itemId: string | null,
    edits: { title?: string; username?: string },
  ): Promise<null> {
    const tab = await this.platform.pages.activeTab();
    const capture = tab ? this.offered(tab.id) : undefined;
    if (!capture) throw new ServiceError("no_offer");
    await this.accept(capture, itemId, edits);
    return null;
  }

  async dismissFromPopup(never: boolean): Promise<null> {
    const tab = await this.platform.pages.activeTab();
    const capture = tab ? this.offered(tab.id) : undefined;
    if (capture) await this.dismiss(capture, never);
    return null;
  }

  private async dismiss(capture: Capture, never: boolean): Promise<void> {
    if (never) {
      const sites = await this.never();
      const site = hostname(capture.tabUrl);
      if (site !== "" && !sites.includes(site)) {
        await this.platform.disk.set(
          "saveNever",
          JSON.stringify([...sites, site]),
        );
      }
    }
    await this.drop(capture.ref.tabId);
  }

  /**
   * "Save login from this page" (D36 §1): reads the sign-in fields the user
   * typed into now, with activeTab, and offers at once.
   */
  async captureActiveTab(): Promise<OfferView> {
    const tab = await this.platform.pages.activeTab();
    if (!tab || !/^https?:/.test(tab.url)) throw new ServiceError("no_page");
    const frames = await this.host.autofill.scanTab(tab.id);
    for (const frame of frames) {
      const watch = this.host.autofill.watchFor(frame);
      if (!watch || (watch.password === null && watch.newPassword.length === 0))
        continue;
      const typed: unknown = await this.platform.pages.send(frame.ref, {
        target: "content",
        kind: "read",
        watch,
      });
      if (
        !isTyped(typed) ||
        (typed.password === "" && typed.newPassword === "")
      )
        continue;
      const capture = await this.capture(
        { ...frame.ref, url: frame.url, tabUrl: frame.tabUrl },
        typed,
        true,
      );
      const view = capture?.token ? this.view(capture) : null;
      if (view) return view;
      throw new ServiceError("already_saved");
    }
    throw new ServiceError("no_password");
  }

  // --- The card (D36 §5) ---

  /** Whether this token is a card's (else it is an inline icon's). */
  owns(token: string): boolean {
    for (const capture of this.captures.values()) {
      if (capture.token === token) return true;
    }
    return false;
  }

  async onInline(
    request: InlineRequest,
    tabId: number,
  ): Promise<OfferView | null> {
    const capture = this.offered(tabId);
    if (capture?.token !== request.token) throw new ServiceError("bad_token");
    switch (request.kind) {
      case "offer": {
        const view = this.view(capture);
        if (!view) await this.drop(tabId);
        return view;
      }
      case "offerSave":
        await this.accept(capture, request.itemId, {});
        return null;
      case "offerDismiss":
      case "close":
        await this.dismiss(capture, false);
        return null;
      case "offerNever":
        await this.dismiss(capture, true);
        return null;
      case "unlock":
        await this.platform.pages.openPopup();
        return null;
      default:
        throw new ServiceError("bad_request");
    }
  }

  // --- Lifetime ---

  /** The vault locked or the user signed out: forget every capture. */
  async clear(): Promise<void> {
    for (const tabId of [...this.captures.keys()]) await this.drop(tabId);
    this.hints.clear();
  }

  onTabRemoved(tabId: number): void {
    this.captures.delete(tabId);
    this.hints.delete(tabId);
  }
}

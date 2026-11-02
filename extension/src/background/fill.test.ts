import { describe, expect, it } from "vitest";
import type { FrameReply } from "../shared/autofill.ts";
import {
  ITEM_PASSWORD,
  PASSWORD,
  TOTP_SECRET,
  fakeFunctions,
  fakePlatform,
  field,
} from "./fakes.ts";
import type { FakeCore, FakeFrame } from "./fakes.ts";
import { Background, ServiceError } from "./service.ts";

const LOGIN_FORM = [
  field(0, { type: "email", name: "email" }),
  field(1, { type: "password", name: "password" }),
];

function frame(patch: Partial<FakeFrame> = {}): FakeFrame {
  return {
    frameId: 0,
    documentId: "doc-top",
    url: "https://example.com/login",
    fields: LOGIN_FORM,
    ...patch,
  };
}

async function setup() {
  const platform = fakePlatform();
  const fns = fakeFunctions();
  const background = await Background.start(fns, platform);
  platform.pages.report = (sender, fields) =>
    background.autofill.onFrameReport(sender, fields);
  await background.handle({
    kind: "signUp",
    email: "ada@example.com",
    password: PASSWORD,
  });
  const core = fns.cores.at(-1) as FakeCore;
  core.addLogin("vault-1", {
    id: "item-1",
    type: "login",
    title: "Example",
    username: "ada",
    password: ITEM_PASSWORD,
    totp: TOTP_SECRET,
    urls: [{ url: "https://example.com" }],
  });
  core.addLogin("vault-1", {
    id: "item-2",
    type: "login",
    title: "Other site",
    username: "bob",
    password: "other-secret",
    urls: [{ url: "https://other.example" }],
  });
  platform.pages.frames = [frame()];
  return { platform, background, core, pages: platform.pages };
}

async function errorOf(promise: Promise<unknown>): Promise<string> {
  try {
    await promise;
  } catch (error) {
    if (error instanceof ServiceError) return error.code;
    throw error;
  }
  throw new Error("no error");
}

const fill = (confirmed = false) =>
  ({ kind: "fill", vaultId: "vault-1", itemId: "item-1", confirmed }) as const;

describe("suggestions", () => {
  it("lists the logins for the active tab, without secrets", async () => {
    const { background } = await setup();
    const reply = await background.handle({ kind: "suggestions" });
    expect(reply).toEqual({
      site: "example.com",
      items: [
        {
          vaultId: "vault-1",
          itemId: "item-1",
          title: "Example",
          username: "ada",
          decision: "fill",
          next: false,
        },
      ],
    });
    expect(JSON.stringify(reply)).not.toContain(ITEM_PASSWORD);
  });

  it("has nothing for browser pages or without a tab", async () => {
    const { background, pages } = await setup();
    pages.tab = { id: 7, url: "chrome://settings" };
    expect(await background.handle({ kind: "suggestions" })).toEqual({
      site: "",
      items: [],
    });
    pages.tab = undefined;
    expect(await background.handle({ kind: "suggestions" })).toEqual({
      site: "",
      items: [],
    });
  });
});

describe("filling from the popup", () => {
  it("sends the values to the one document that reported the form", async () => {
    const { background, pages } = await setup();
    expect(await background.handle(fill())).toEqual({ copiedCode: false });
    expect(pages.sent).toEqual([
      {
        target: { tabId: 7, frameId: 0, documentId: "doc-top" },
        message: {
          target: "content",
          kind: "fill",
          fills: [
            { index: 0, value: "ada" },
            { index: 1, value: ITEM_PASSWORD },
          ],
          scroll: true,
        },
      },
    ]);
  });

  it("never sends content scripts other items or the vault list", async () => {
    const { background, pages } = await setup();
    await background.handle(fill());
    const text = JSON.stringify(pages.sent);
    for (const secret of ["other-secret", "bob", "Other site", TOTP_SECRET]) {
      expect(text).not.toContain(secret);
    }
  });

  it("records the step so the next page of the login offers the same item", async () => {
    const { background, pages } = await setup();
    pages.frames = [frame({ fields: [field(0, { type: "email" })] })];
    await background.handle(fill());
    const reply = await background.handle({ kind: "suggestions" });
    expect(reply.items[0]).toMatchObject({ itemId: "item-1", next: true });
  });

  it("asks before filling on an http page, and fills once confirmed", async () => {
    const { background, pages, core } = await setup();
    core.addLogin("vault-1", {
      id: "item-1",
      type: "login",
      title: "Example (http)",
      username: "ada",
      password: ITEM_PASSWORD,
      urls: [{ url: "http://plain.example" }],
    });
    pages.tab = { id: 7, url: "http://plain.example/login" };
    pages.frames = [frame({ url: "http://plain.example/login" })];
    expect(await errorOf(background.handle(fill()))).toBe("needs_confirm");
    expect(pages.sent).toEqual([]);
    await background.handle(fill(true));
    expect(pages.sent).toHaveLength(1);
  });

  it("refuses a login form in a frame from another site", async () => {
    const { background, pages } = await setup();
    pages.frames = [
      frame({ fields: [] }),
      frame({
        frameId: 3,
        documentId: "doc-evil",
        url: "https://evil.example/frame",
      }),
    ];
    expect(await errorOf(background.handle(fill()))).toBe("refused");
    expect(pages.sent).toEqual([]);
  });

  it("fills a login form in a frame of the same site", async () => {
    const { background, pages } = await setup();
    pages.frames = [
      frame({ fields: [] }),
      frame({
        frameId: 3,
        documentId: "doc-frame",
        url: "https://example.com/embedded-login",
      }),
    ];
    await background.handle(fill());
    expect(pages.sent[0]?.target).toEqual({
      tabId: 7,
      frameId: 3,
      documentId: "doc-frame",
    });
  });

  it("says so when there is no form", async () => {
    const { background, pages } = await setup();
    pages.frames = [frame({ fields: [field(0, {})] })];
    expect(await errorOf(background.handle(fill()))).toBe("no_form");
  });

  it("fills nothing when the page changed or the field is hidden", async () => {
    const { background, pages } = await setup();
    pages.gone.add("doc-top");
    expect(await errorOf(background.handle(fill()))).toBe("page_changed");
    pages.gone.clear();
    pages.fillReply = { ok: false, reason: "covered" };
    expect(await errorOf(background.handle(fill()))).toBe("field_hidden");
  });

  it("fills a code field the engine is sure of", async () => {
    const { background, pages } = await setup();
    pages.frames = [
      frame({ fields: [field(0, { autocomplete: "one-time-code" })] }),
    ];
    await background.handle(fill());
    const message = pages.sent[0]?.message;
    expect(message).toMatchObject({ fills: [{ index: 0, value: "287082" }] });
  });

  it("copies the code when the field is not surely a code field", async () => {
    const { background, pages, platform } = await setup();
    pages.frames = [frame({ fields: [field(0, { name: "otp" })] })];
    expect(await background.handle(fill())).toEqual({ copiedCode: true });
    expect(platform.clipboardText.value).toBe("287082");
    expect(platform.alarmsAt.get("clipboard")).toBe(
      platform.clock.now + 30_000,
    );
    expect(pages.sent).toEqual([]);
  });

  it("needs the vault unlocked", async () => {
    const { background } = await setup();
    await background.handle({ kind: "lock" });
    expect(await errorOf(background.handle(fill()))).toBe("locked");
    expect(await errorOf(background.handle({ kind: "suggestions" }))).toBe(
      "locked",
    );
  });
});

describe("the shortcut", () => {
  it("fills the one sure match without opening the popup", async () => {
    const { background, pages } = await setup();
    await background.autofill.onCommand(pages.tab);
    expect(pages.popups).toBe(0);
    expect(pages.sent).toHaveLength(1);
  });

  it("opens the popup when locked, with no match, or with several", async () => {
    const { background, pages, core } = await setup();
    pages.tab = { id: 7, url: "https://nothing.example/" };
    await background.autofill.onCommand(pages.tab);
    expect(pages.popups).toBe(1);

    pages.tab = { id: 7, url: "https://example.com/login" };
    core.addLogin("vault-1", {
      id: "item-3",
      type: "login",
      title: "Example 2",
      username: "ada2",
      password: "x",
      urls: [{ url: "https://example.com" }],
    });
    await background.autofill.onCommand(pages.tab);
    expect(pages.popups).toBe(2);

    await background.handle({ kind: "lock" });
    await background.autofill.onCommand(pages.tab);
    expect(pages.popups).toBe(3);
    expect(pages.sent).toEqual([]);
  });

  it("opens the popup instead of filling with a warning", async () => {
    const { background, pages, core } = await setup();
    core.addLogin("vault-1", {
      id: "item-4",
      type: "login",
      title: "Plain",
      username: "ada",
      password: "x",
      urls: [{ url: "http://plain.example" }],
    });
    pages.tab = { id: 7, url: "http://plain.example/" };
    await background.autofill.onCommand(pages.tab);
    expect(pages.popups).toBe(1);
    expect(pages.sent).toEqual([]);
  });

  it("opens the popup when the fill fails", async () => {
    const { background, pages } = await setup();
    pages.fillReply = { ok: false, reason: "covered" };
    await background.autofill.onCommand(pages.tab);
    expect(pages.popups).toBe(1);
  });
});

describe("the inline icon", () => {
  const sender = {
    tabId: 7,
    frameId: 0,
    documentId: "doc-top",
    url: "https://example.com/login",
    tabUrl: "https://example.com/login",
  };

  async function withIcon() {
    const env = await setup();
    env.pages.siteAccess = true;
    expect(
      await env.background.handle({ kind: "setInlineIcon", on: true }),
    ).toBe(true);
    const reply = await env.background.autofill.onFrameReport(
      sender,
      LOGIN_FORM,
    );
    if (reply.token === null) throw new Error("no token");
    return { ...env, token: reply.token, reply };
  }

  const inline = (token: string, patch: object) => ({
    target: "background" as const,
    from: "inline" as const,
    token,
    index: 0,
    ...patch,
  });

  it("is off until turned on with site access", async () => {
    const { background, pages } = await setup();
    const none: FrameReply = { token: null, icons: [] };
    expect(await background.autofill.onFrameReport(sender, LOGIN_FORM)).toEqual(
      none,
    );
    // Turning it on without the permission does nothing.
    expect(await background.handle({ kind: "setInlineIcon", on: true })).toBe(
      false,
    );
    expect(pages.inlineScripts).toBe(false);
    // Site access alone (a self-hosted server) does not turn it on.
    pages.siteAccess = true;
    expect(await background.autofill.onFrameReport(sender, LOGIN_FORM)).toEqual(
      none,
    );
  });

  it("shows on the login fields once on, with a fresh token", async () => {
    const { reply, pages, background } = await withIcon();
    expect(pages.inlineScripts).toBe(true);
    expect(reply.icons).toEqual([0, 1]);
    expect(await background.handle({ kind: "inlineIcon" })).toBe(true);
    const again = await background.autofill.onFrameReport(sender, LOGIN_FORM);
    expect(again.token).not.toBe(reply.token);
  });

  it("lists titles and usernames only, and opens the menu", async () => {
    const { background, token, pages } = await withIcon();
    const menu = await background.autofill.onInline(
      inline(token, { kind: "menu" }) as never,
      7,
    );
    expect(menu).toEqual({
      state: "unlocked",
      items: [
        {
          vaultId: "vault-1",
          itemId: "item-1",
          title: "Example",
          username: "ada",
          decision: "fill",
          next: false,
        },
      ],
    });
    expect(JSON.stringify(menu)).not.toContain(ITEM_PASSWORD);
    expect(pages.sent.at(-1)?.message).toEqual({
      target: "content",
      kind: "menu",
      index: 0,
      open: true,
    });
  });

  it("refuses a token from another tab or an unknown one", async () => {
    const { background, token } = await withIcon();
    expect(
      await errorOf(
        background.autofill.onInline(
          inline(token, { kind: "menu" }) as never,
          8,
        ),
      ),
    ).toBe("bad_token");
    expect(
      await errorOf(
        background.autofill.onInline(
          inline("forged", { kind: "menu" }) as never,
          7,
        ),
      ),
    ).toBe("bad_token");
  });

  it("says locked instead of listing when the vault is locked", async () => {
    const { background, token } = await withIcon();
    await background.handle({ kind: "lock" });
    expect(
      await background.autofill.onInline(
        inline(token, { kind: "menu" }) as never,
        7,
      ),
    ).toEqual({ state: "locked" });
  });

  it("closes the menu, then fills the chosen login", async () => {
    const { background, token, pages } = await withIcon();
    await background.autofill.onInline(
      inline(token, {
        kind: "fill",
        vaultId: "vault-1",
        itemId: "item-1",
      }) as never,
      7,
    );
    const kinds = pages.sent.map((s) => s.message.kind);
    expect(kinds).toEqual(["menu", "fill"]);
    expect(pages.sent[1]?.message).toMatchObject({ scroll: false });
  });

  it("refuses another site's login from the menu", async () => {
    const { background, token, pages } = await withIcon();
    expect(
      await errorOf(
        background.autofill.onInline(
          inline(token, {
            kind: "fill",
            vaultId: "vault-1",
            itemId: "item-2",
          }) as never,
          7,
        ),
      ),
    ).toBe("refused");
    expect(pages.sent.filter((s) => s.message.kind === "fill")).toEqual([]);
  });

  it("stays away from a site after Don't show here", async () => {
    const { background, token, pages } = await withIcon();
    await background.autofill.onInline(
      inline(token, { kind: "hide" }) as never,
      7,
    );
    expect(pages.sent.at(-1)?.message.kind).toBe("removeIcons");
    expect(
      (await background.autofill.onFrameReport(sender, LOGIN_FORM)).token,
    ).toBeNull();
  });

  it("forgets the tab's tokens when it closes", async () => {
    const { background, token } = await withIcon();
    background.autofill.onTabRemoved(7);
    expect(
      await errorOf(
        background.autofill.onInline(
          inline(token, { kind: "menu" }) as never,
          7,
        ),
      ),
    ).toBe("bad_token");
  });

  it("turns off: scripts removed and icons taken down", async () => {
    const { background, pages, token } = await withIcon();
    expect(await background.handle({ kind: "setInlineIcon", on: false })).toBe(
      false,
    );
    expect(pages.inlineScripts).toBe(false);
    expect(pages.sent.at(-1)?.message.kind).toBe("removeIcons");
    expect(
      await errorOf(
        background.autofill.onInline(
          inline(token, { kind: "menu" }) as never,
          7,
        ),
      ),
    ).toBe("bad_token");
  });

  it("stops the scripts when site access is taken back", async () => {
    const { background, pages } = await withIcon();
    pages.siteAccess = false;
    await background.autofill.dropInlineIfNoAccess();
    expect(pages.inlineScripts).toBe(false);
    expect(await background.handle({ kind: "inlineIcon" })).toBe(false);
  });
});

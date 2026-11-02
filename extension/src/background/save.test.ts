import { describe, expect, it } from "vitest";
import type { Typed } from "../shared/autofill.ts";
import {
  ITEM_PASSWORD,
  PASSWORD,
  fakeFunctions,
  fakePlatform,
  field,
} from "./fakes.ts";
import type { FakeCore } from "./fakes.ts";
import type { FrameSender } from "./platform.ts";
import { CAPTURE_TTL_MS, WORKED_WITHIN_MS } from "./save.ts";
import { Background, ServiceError } from "./service.ts";

const FORM = [
  field(0, { type: "email", name: "email" }),
  field(1, { type: "password", name: "password" }),
];

function at(url: string, documentId: string, frameId = 0): FrameSender {
  return { tabId: 7, frameId, documentId, url, tabUrl: url };
}

const typed = (
  username: string,
  password: string,
  newPassword = "",
): Typed => ({
  username,
  password,
  newPassword,
});

async function setup({ enabled = true } = {}) {
  const platform = fakePlatform();
  const fns = fakeFunctions();
  const background = await Background.start(fns, platform);
  const pages = platform.pages;
  pages.report = (sender, fields) =>
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
    urls: [{ url: "https://example.com" }],
  });
  if (enabled) {
    pages.siteAccess = true;
    await background.handle({ kind: "setInlineIcon", on: true });
  }
  pages.tab = { id: 7, url: "https://new.example/login" };
  return { platform, pages, background, core };
}

/** Load a page (its content script reports), submit, then load the next. */
async function signIn(
  env: Awaited<ReturnType<typeof setup>>,
  values: Typed,
  url = "https://new.example/login",
) {
  const { background } = env;
  await background.autofill.onFrameReport(at(url, "doc-1"), FORM);
  await background.saver.onSubmitted(at(url, "doc-1"), values);
  await background.autofill.onFrameReport(
    at(url.replace("/login", "/home"), "doc-2"),
    [],
  );
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

describe("after a sign-in that worked", () => {
  it("offers to save a new login, on the next page, without the password", async () => {
    const env = await setup();
    const { background, pages } = env;
    await signIn(env, typed("ada@new.example", "fresh-secret"));

    expect(pages.badges.get(7)).toBe("1");
    const shown = pages.sent.find((s) => s.message.kind === "showOffer");
    expect(shown?.target).toEqual({
      tabId: 7,
      frameId: 0,
      documentId: "doc-2",
    });

    const view = await background.handle({ kind: "saveOffer" });
    expect(view).toEqual({
      site: "new.example",
      action: "save",
      username: "ada@new.example",
      title: "new.example",
      insecure: false,
      candidates: [],
      locked: false,
    });
    expect(JSON.stringify([view, pages.sent])).not.toContain("fresh-secret");
  });

  it("saves it, with the title and username the popup changed", async () => {
    const env = await setup();
    const { background, pages, core, platform } = env;
    await signIn(env, typed("ada@new.example", "fresh-secret"));
    const before = platform.disk.snapshot;
    await background.handle({
      kind: "acceptOffer",
      itemId: null,
      title: "New site",
      username: "ada",
    });
    const saved = JSON.parse(core.items("vault-1").at(-1) ?? "{}") as {
      title: string;
      login: { username: string; password: string; urls: { url: string }[] };
    };
    expect(saved.title).toBe("New site");
    expect(saved.login).toMatchObject({
      username: "ada",
      password: "fresh-secret",
      urls: [{ url: "https://new.example" }],
    });
    expect(platform.disk.snapshot).not.toBe(before);
    expect(pages.badges.get(7)).toBe("");
    expect(pages.sent.at(-1)?.message.kind).toBe("hideOffer");
    expect(await background.handle({ kind: "saveOffer" })).toBeNull();
  });

  it("offers an update when the password changed, and keeps the old one", async () => {
    const env = await setup();
    const { background, core, pages } = env;
    pages.tab = { id: 7, url: "https://example.com/login" };
    await signIn(env, typed("ADA ", "changed!"), "https://example.com/login");
    const view = await background.handle({ kind: "saveOffer" });
    expect(view).toMatchObject({
      action: "update",
      username: "ADA",
      candidates: [
        {
          vaultId: "vault-1",
          itemId: "item-1",
          title: "Example",
          username: "ada",
        },
      ],
    });
    await background.handle({ kind: "acceptOffer", itemId: "item-1" });
    expect(core.history.get("item-1")).toEqual([ITEM_PASSWORD]);
    expect(
      await background.autofill.onFrameReport(
        at("https://example.com/x", "d"),
        [],
      ),
    ).toBeDefined();
  });

  it("refuses an update to a login that is not a candidate", async () => {
    const env = await setup();
    const { background, pages } = env;
    pages.tab = { id: 7, url: "https://example.com/login" };
    await signIn(env, typed("ada", "changed!"), "https://example.com/login");
    expect(
      await errorOf(
        background.handle({ kind: "acceptOffer", itemId: "item-9" }),
      ),
    ).toBe("unknown_item");
  });

  it("counts the password field going away as worked (single-page apps)", async () => {
    const env = await setup();
    const { background, pages } = env;
    const page = at("https://new.example/login", "doc-1");
    await background.autofill.onFrameReport(page, FORM);
    await background.saver.onSubmitted(page, typed("ada", "spa-secret"));
    expect(pages.badges.get(7)).toBeUndefined();
    await background.saver.onFormGone(page);
    expect(pages.badges.get(7)).toBe("1");
  });

  it("joins the username of a multi-step login's first page", async () => {
    const env = await setup();
    const { background } = env;
    const one = at("https://new.example/step1", "doc-1");
    await background.autofill.onFrameReport(one, FORM);
    await background.saver.onSubmitted(one, typed("ada2", ""));
    const two = at("https://new.example/step2", "doc-2");
    await background.autofill.onFrameReport(two, FORM);
    await background.saver.onSubmitted(two, typed("", "step-secret"));
    await background.autofill.onFrameReport(
      at("https://new.example/home", "doc-3"),
      [],
    );
    expect(await background.handle({ kind: "saveOffer" })).toMatchObject({
      action: "save",
      username: "ada2",
    });
  });
});

describe("no offer", () => {
  it("after a failed sign-in (the same page stays)", async () => {
    const env = await setup();
    const { background, pages, platform } = env;
    const page = at("https://new.example/login", "doc-1");
    await background.autofill.onFrameReport(page, FORM);
    await background.saver.onSubmitted(page, typed("ada", "wrong"));
    // The same document reports again (the page re-rendered its error).
    await background.autofill.onFrameReport(page, FORM);
    // Too late for a navigation to count.
    platform.clock.now += WORKED_WITHIN_MS + 1;
    await background.autofill.onFrameReport(
      at("https://new.example/other", "doc-2"),
      [],
    );
    await background.saver.onFormGone(page);
    expect(pages.badges.get(7)).toBeUndefined();
    expect(await background.handle({ kind: "saveOffer" })).toBeNull();
  });

  it("when navigating to another site", async () => {
    const env = await setup();
    const { background, pages } = env;
    const page = at("https://new.example/login", "doc-1");
    await background.autofill.onFrameReport(page, FORM);
    await background.saver.onSubmitted(page, typed("ada", "x"));
    await background.autofill.onFrameReport(
      at("https://elsewhere.test/", "doc-2"),
      [],
    );
    expect(pages.badges.get(7)).toBeUndefined();
  });

  it("for credentials already saved as they are", async () => {
    const env = await setup();
    const { background, pages } = env;
    pages.tab = { id: 7, url: "https://example.com/login" };
    await signIn(env, typed("ada", ITEM_PASSWORD), "https://example.com/login");
    expect(pages.badges.get(7)).toBeUndefined();
    expect(await background.handle({ kind: "saveOffer" })).toBeNull();
  });

  it("when Fill and save is off", async () => {
    const env = await setup({ enabled: false });
    await signIn(env, typed("ada", "x"));
    expect(env.pages.badges.get(7)).toBeUndefined();
  });

  it("on a site marked Never", async () => {
    const env = await setup();
    const { background, pages, platform } = env;
    await signIn(env, typed("ada", "x"));
    await background.handle({ kind: "dismissOffer", never: true });
    expect(platform.disk.values.get("saveNever")).toBe('["new.example"]');
    expect(pages.badges.get(7)).toBe("");
    await signIn(env, typed("bob", "y"));
    expect(pages.badges.get(7)).toBe("");
  });

  it("after five minutes", async () => {
    const env = await setup();
    const { background, platform } = env;
    await signIn(env, typed("ada", "x"));
    platform.clock.now += CAPTURE_TTL_MS + 1;
    expect(await background.handle({ kind: "saveOffer" })).toBeNull();
  });

  it("after the vault locks", async () => {
    const env = await setup();
    const { background, pages } = env;
    await signIn(env, typed("ada", "x"));
    await background.handle({ kind: "lock" });
    expect(pages.badges.get(7)).toBe("");
    await background.handle({ kind: "unlock", password: PASSWORD });
    expect(await background.handle({ kind: "saveOffer" })).toBeNull();
  });

  it("and the capture is never in storage", async () => {
    const env = await setup();
    const { platform } = env;
    await signIn(env, typed("ada", "never-stored"));
    const stored =
      JSON.stringify([...platform.disk.values]) +
      new TextDecoder().decode(platform.disk.snapshot) +
      new TextDecoder().decode(platform.sessionBytes.value);
    expect(stored).not.toContain("never-stored");
  });
});

describe("a sign-in while locked", () => {
  it("offers once unlocked", async () => {
    const env = await setup();
    const { background } = env;
    await background.handle({ kind: "lock" });
    await signIn(env, typed("ada", "x"));
    expect(await background.handle({ kind: "saveOffer" })).toMatchObject({
      locked: true,
    });
    await background.handle({ kind: "unlock", password: PASSWORD });
    expect(await background.handle({ kind: "saveOffer" })).toMatchObject({
      action: "save",
      locked: false,
    });
  });
});

describe("the card", () => {
  async function card() {
    const env = await setup();
    await signIn(env, typed("ada", "card-secret"));
    const shown = env.pages.sent.find((s) => s.message.kind === "showOffer");
    const token =
      shown?.message.kind === "showOffer" ? shown.message.token : "";
    return { ...env, token };
  }

  const request = (token: string, patch: object) =>
    ({
      target: "background",
      from: "inline",
      token,
      index: 0,
      ...patch,
    }) as never;

  it("shows the offer for its own tab only", async () => {
    const { background, token } = await card();
    expect(background.saver.owns(token)).toBe(true);
    const view = await background.saver.onInline(
      request(token, { kind: "offer" }),
      7,
    );
    expect(view).toMatchObject({ action: "save", site: "new.example" });
    expect(JSON.stringify(view)).not.toContain("card-secret");
    expect(
      await errorOf(
        background.saver.onInline(request(token, { kind: "offer" }), 8),
      ),
    ).toBe("bad_token");
    expect(
      await errorOf(
        background.saver.onInline(request("forged", { kind: "offer" }), 7),
      ),
    ).toBe("bad_token");
  });

  it("saves, and is gone afterwards", async () => {
    const { background, token, core } = await card();
    await background.saver.onInline(
      request(token, { kind: "offerSave", itemId: null }),
      7,
    );
    expect(core.items("vault-1").some((j) => j.includes("card-secret"))).toBe(
      true,
    );
    expect(background.saver.owns(token)).toBe(false);
  });

  it("Not now drops the offer", async () => {
    const { background, token, pages } = await card();
    await background.saver.onInline(
      request(token, { kind: "offerDismiss" }),
      7,
    );
    expect(pages.badges.get(7)).toBe("");
    expect(await background.handle({ kind: "saveOffer" })).toBeNull();
  });
});

describe("Save login from this page", () => {
  it("reads the typed fields now and offers at once", async () => {
    const env = await setup({ enabled: false });
    const { background, pages } = env;
    pages.frames = [
      {
        frameId: 0,
        documentId: "doc-1",
        url: "https://new.example/login",
        fields: FORM,
      },
    ];
    pages.typed = typed("ada", "typed-secret");
    const view = await background.handle({ kind: "captureNow" });
    expect(view).toMatchObject({ action: "save", username: "ada" });
    const read = pages.sent.find((s) => s.message.kind === "read");
    expect(read?.message).toEqual({
      target: "content",
      kind: "read",
      watch: { username: 0, password: 1, newPassword: [] },
    });
  });

  it("says so when nothing is typed or it is already saved", async () => {
    const env = await setup({ enabled: false });
    const { background, pages } = env;
    pages.frames = [
      {
        frameId: 0,
        documentId: "doc-1",
        url: "https://example.com/login",
        fields: FORM,
      },
    ];
    pages.tab = { id: 7, url: "https://example.com/login" };
    pages.typed = typed("ada", "");
    expect(await errorOf(background.handle({ kind: "captureNow" }))).toBe(
      "no_password",
    );
    pages.typed = typed("ada", ITEM_PASSWORD);
    expect(await errorOf(background.handle({ kind: "captureNow" }))).toBe(
      "already_saved",
    );
  });
});

describe("watching fields", () => {
  it("asks content scripts to read only the sign-in fields, when on", async () => {
    const env = await setup();
    const reply = await env.background.autofill.onFrameReport(
      at("https://new.example/signup", "doc-1"),
      [
        field(0, { type: "email" }),
        field(1, { type: "password", autocomplete: "new-password" }),
        field(2, { type: "password", autocomplete: "new-password" }),
        field(3, { name: "search" }),
      ],
    );
    expect(reply.watch).toEqual({
      username: 0,
      password: null,
      newPassword: [1, 2],
    });
    const off = await setup({ enabled: false });
    const none = await off.background.autofill.onFrameReport(
      at("https://new.example/signup", "doc-1"),
      FORM,
    );
    expect(none.watch).toBeNull();
  });
});

describe("message order", () => {
  it("still offers when the next page reported before the submit arrived", async () => {
    const env = await setup();
    const { background, pages } = env;
    await background.autofill.onFrameReport(
      at("https://new.example/login", "doc-1"),
      FORM,
    );
    await background.autofill.onFrameReport(
      at("https://new.example/home", "doc-2"),
      [],
    );
    await background.saver.onSubmitted(
      at("https://new.example/login", "doc-1"),
      typed("ada", "late-message"),
    );
    expect(pages.badges.get(7)).toBe("1");
  });
});

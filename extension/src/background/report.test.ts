import { describe, expect, it } from "vitest";
import { HOSTED_SERVER } from "../shared/config.ts";
import { browserName } from "./chrome.ts";
import { PASSWORD, fakeFunctions, fakePlatform, field } from "./fakes.ts";
import type { FakeCore } from "./fakes.ts";
import { REPORT_PATH } from "./report.ts";
import { Background, ServiceError } from "./service.ts";

const FORM = [
  field(0, { type: "email", name: "email" }),
  field(1, { type: "password", name: "password" }),
];

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
  platform.pages.tab = { id: 7, url: "https://shop.example.com/login?x=1" };
  platform.pages.frames = [
    {
      frameId: 0,
      documentId: "d0",
      url: "https://shop.example.com/login?x=1",
      fields: FORM,
    },
    {
      frameId: 2,
      documentId: "d2",
      url: "https://auth.example.com/frame",
      fields: [],
    },
    {
      frameId: 3,
      documentId: "d3",
      url: "https://ads.other.test/x",
      fields: [],
    },
  ];
  platform.network.status = 202;
  return { background, platform, core };
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

const posted = (platform: ReturnType<typeof fakePlatform>) =>
  platform.requests.filter((r) => r.url === HOSTED_SERVER + REPORT_PATH);

describe("the preview", () => {
  it("gives the core each frame's fields, relation and fill targets, and sends nothing", async () => {
    const { background, platform, core } = await setup();
    const preview = await background.handle({
      kind: "reportPreview",
      problem: "wrong_field",
    });
    expect(core.reports).toHaveLength(1);
    const call = core.reports[0];
    expect(call?.pageUrl).toBe("https://shop.example.com/login?x=1");
    expect(call?.problem).toBe("wrong_field");
    const frames = JSON.parse(call?.framesJson ?? "[]") as {
      relation: string;
      fill: number[];
    }[];
    expect(frames.map((f) => f.relation)).toEqual([
      "top",
      "same_site",
      "other_site",
    ]);
    expect(frames[0]?.fill).toEqual([0, 1]);
    expect(preview).toMatchObject({
      site: "https://shop.example.com",
      forms: 1,
      fields: 2,
      kinds: ["current-password", "username"],
    });
    expect(JSON.parse(preview.json)).toMatchObject({
      problem: "wrong_field",
      matched: true,
    });
    expect(posted(platform)).toEqual([]);
  });

  it("works while locked, without saying whether a login matched", async () => {
    const { background } = await setup();
    await background.handle({ kind: "lock" });
    const preview = await background.handle({
      kind: "reportPreview",
      problem: "other",
    });
    expect(JSON.parse(preview.json)).toMatchObject({ matched: null });
  });

  it("needs a web page", async () => {
    const { background, platform } = await setup();
    platform.pages.tab = { id: 7, url: "chrome://settings" };
    expect(
      await errorOf(
        background.handle({ kind: "reportPreview", problem: "other" }),
      ),
    ).toBe("no_page");
  });
});

describe("sending", () => {
  it("posts exactly the preview, anonymously", async () => {
    const { background, platform } = await setup();
    const preview = await background.handle({
      kind: "reportPreview",
      problem: "nothing_filled",
    });
    await background.handle({ kind: "reportSend" });
    const [request] = posted(platform);
    expect(request?.init).toMatchObject({
      method: "POST",
      body: preview.json,
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
    });
    expect(request?.init.headers).toEqual({
      "content-type": "application/json",
    });
    // Sent once: a second Send needs a new look first.
    expect(await errorOf(background.handle({ kind: "reportSend" }))).toBe(
      "no_report",
    );
  });

  it("goes to the hosted service even when self-hosting", async () => {
    const { background, platform } = await setup();
    await background.handle({ kind: "signOut" });
    await background.handle({
      kind: "setSettings",
      settings: { serverUrl: "https://vault.example.org", autoLockMinutes: 15 },
    });
    await background.handle({ kind: "reportPreview", problem: "other" });
    await background.handle({ kind: "reportSend" });
    expect(posted(platform)).toHaveLength(1);
  });

  it("says so when rate-limited, offline or refused", async () => {
    const { background, platform } = await setup();
    await background.handle({ kind: "reportPreview", problem: "other" });
    platform.network.status = 429;
    expect(await errorOf(background.handle({ kind: "reportSend" }))).toBe(
      "rate_limited",
    );
    platform.network.status = 400;
    expect(await errorOf(background.handle({ kind: "reportSend" }))).toBe(
      "server",
    );
    platform.network.down = true;
    expect(await errorOf(background.handle({ kind: "reportSend" }))).toBe(
      "network",
    );
  });

  it("forgets the preview when the tab closes", async () => {
    const { background } = await setup();
    await background.handle({ kind: "reportPreview", problem: "other" });
    background.reporter.onTabRemoved(7);
    expect(await errorOf(background.handle({ kind: "reportSend" }))).toBe(
      "no_report",
    );
  });
});

describe("browserName", () => {
  it("reads the browser and its major version from the user agent", () => {
    const chrome =
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.7339.80 Safari/537.36";
    expect(browserName(chrome)).toBe("Chrome 140");
    expect(browserName(`${chrome} Edg/140.0.3485.54`)).toBe("Edge 140");
    expect(browserName(`${chrome} OPR/125.0.0.0`)).toBe("Opera 125");
    expect(browserName("curl/8")).toBe("Other");
  });
});

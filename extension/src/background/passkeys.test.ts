import { afterEach, describe, expect, it, vi } from "vitest";
import type { PasskeyAnswer, PasskeyPrompt } from "../shared/passkeys.ts";
import { PASSWORD, fakeFunctions, fakePlatform } from "./fakes.ts";
import type { FakeCore } from "./fakes.ts";
import type { FrameSender } from "./platform.ts";
import { Background } from "./service.ts";

const PAGE = "https://www.example.com/account";

function from(url = PAGE, frameId = 0, tabUrl = PAGE): FrameSender {
  return { tabId: 7, frameId, documentId: "doc", url, tabUrl };
}

const create = (patch: object = {}) =>
  JSON.stringify({
    challenge: "AAAA",
    rp: { id: "example.com", name: "Example" },
    user: { id: "AA", name: "ada", displayName: "Ada" },
    pubKeyCredParams: [{ type: "public-key", alg: -7 }],
    ...patch,
  });

const get = (patch: object = {}) =>
  JSON.stringify({ challenge: "AAAA", rpId: "example.com", ...patch });

async function setup({ enabled = true } = {}) {
  const platform = fakePlatform();
  const fns = fakeFunctions();
  const background = await Background.start(fns, platform);
  await background.handle({
    kind: "signUp",
    email: "ada@example.com",
    password: PASSWORD,
  });
  const core = fns.cores.at(-1) as FakeCore;
  if (enabled) {
    platform.pages.siteAccess = true;
    expect(await background.handle({ kind: "setPasskeys", on: true })).toBe(
      true,
    );
  }
  return { background, platform, pages: platform.pages, core };
}

/** The id of the confirmation window's request, from its URL. */
function windowId(pages: { windows: string[] }): string {
  const url = pages.windows.at(-1) ?? "";
  return url.slice(url.indexOf("#") + 1);
}

const w = (id: string, patch: object) =>
  ({ target: "background", from: "passkeyWindow", id, ...patch }) as never;

/** Starts a call and waits until its window opened. */
async function call(
  env: Awaited<ReturnType<typeof setup>>,
  op: "create" | "get",
  request: string,
  sender = from(),
): Promise<{ answer: Promise<PasskeyAnswer>; id: string }> {
  const before = env.pages.windows.length;
  const answer = env.background.passkeys.onCall(sender, op, request);
  await vi.waitFor(() => {
    expect(env.pages.windows.length).toBe(before + 1);
  });
  return { answer, id: windowId(env.pages) };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("what goes back to the browser without asking", () => {
  it("everything, while the switch is off", async () => {
    const env = await setup({ enabled: false });
    expect(
      await env.background.passkeys.onCall(from(), "create", create()),
    ).toEqual({
      fallback: true,
    });
    expect(env.pages.windows).toEqual([]);
  });

  it("a sign-in when the vault has no passkey for the site, or is locked", async () => {
    const env = await setup();
    expect(await env.background.passkeys.onCall(from(), "get", get())).toEqual({
      fallback: true,
    });
    env.core.passkeysMade.push({
      vaultId: "vault-1",
      host: "example.com",
      user: "ada",
      id: "pk-1",
    });
    await env.background.handle({ kind: "lock" });
    expect(await env.background.passkeys.onCall(from(), "get", get())).toEqual({
      fallback: true,
    });
    expect(env.pages.windows).toEqual([]);
  });

  it("a create without ES256, or excluding one of ours", async () => {
    const env = await setup();
    expect(
      await env.background.passkeys.onCall(
        from(),
        "create",
        create({ pubKeyCredParams: [{ type: "public-key", alg: -257 }] }),
      ),
    ).toEqual({ fallback: true });
    env.core.passkeysMade.push({
      vaultId: "vault-1",
      host: "example.com",
      user: "ada",
      id: "pk-1",
    });
    expect(
      await env.background.passkeys.onCall(
        from(),
        "create",
        create({ excludeCredentials: [{ type: "public-key", id: "pk-1" }] }),
      ),
    ).toEqual({ fallback: true });
    expect(env.pages.windows).toEqual([]);
  });

  it("a call from a frame of another site", async () => {
    const env = await setup();
    expect(
      await env.background.passkeys.onCall(
        from("https://evil.example/f", 3),
        "create",
        create(),
      ),
    ).toEqual({ fallback: true });
  });
});

describe("refusals", () => {
  it("an RP id that is not the page's is a SecurityError", async () => {
    const env = await setup();
    expect(
      await env.background.passkeys.onCall(
        from("https://evil.example/"),
        "create",
        create(),
      ),
    ).toEqual({ error: "SecurityError" });
  });
});

describe("creating a passkey", () => {
  it("asks in a window, then creates and stores it", async () => {
    const env = await setup();
    const { answer, id } = await call(env, "create", create());
    expect(env.pages.windows.at(-1)).toBe(`passkey.html#${id}`);
    const prompt = await env.background.passkeys.onWindow(
      w(id, { kind: "prompt" }),
    );
    expect(prompt).toEqual({
      state: "create",
      site: "www.example.com",
      rpId: "example.com",
      userName: "ada",
    });
    await env.background.passkeys.onWindow(w(id, { kind: "create" }));
    const result = await answer;
    expect(result).toMatchObject({
      result: expect.stringContaining('"id":"pk-1"') as unknown,
    });
    expect(env.core.passkeysMade).toEqual([
      { vaultId: "vault-1", host: "example.com", user: "ada", id: "pk-1" },
    ]);
    expect(env.pages.closed).toEqual([1]);
    expect(env.platform.disk.snapshot).toBeDefined();
  });

  it("unlocks first in the window when the vault is locked", async () => {
    const env = await setup();
    await env.background.handle({ kind: "lock" });
    const { answer, id } = await call(env, "create", create());
    expect(
      await env.background.passkeys.onWindow(w(id, { kind: "prompt" })),
    ).toEqual({
      state: "locked",
      site: "www.example.com",
    });
    await expect(
      env.background.passkeys.onWindow(
        w(id, { kind: "unlock", password: "wrong" }),
      ),
    ).rejects.toThrow();
    const after = await env.background.passkeys.onWindow(
      w(id, { kind: "unlock", password: PASSWORD }),
    );
    expect(after).toMatchObject({ state: "create", rpId: "example.com" });
    await env.background.passkeys.onWindow(w(id, { kind: "cancel" }));
    expect(await answer).toEqual({ error: "NotAllowedError" });
  });
});

describe("signing in", () => {
  it("lets the user pick the passkey, then signs", async () => {
    const env = await setup();
    env.core.passkeysMade.push(
      { vaultId: "vault-1", host: "example.com", user: "ada", id: "pk-1" },
      { vaultId: "vault-1", host: "example.com", user: "bob", id: "pk-2" },
    );
    const { answer, id } = await call(env, "get", get());
    const prompt = (await env.background.passkeys.onWindow(
      w(id, { kind: "prompt" }),
    )) as Extract<PasskeyPrompt, { state: "get" }>;
    expect(prompt.candidates.map((c) => c.userName)).toEqual(["ada", "bob"]);
    expect(JSON.stringify(prompt)).not.toMatch(/private|key":/i);
    const bob = prompt.candidates[1];
    await env.background.passkeys.onWindow(
      w(id, {
        kind: "get",
        vaultId: bob?.vaultId,
        itemId: bob?.itemId,
        credentialId: bob?.credentialId,
      }),
    );
    expect(await answer).toEqual({
      result: JSON.stringify({
        id: "pk-2",
        type: "public-key",
        response: { signature: "sig" },
      }),
    });
  });
});

describe("ending without a passkey", () => {
  async function pending() {
    const env = await setup();
    env.core.passkeysMade.push({
      vaultId: "vault-1",
      host: "example.com",
      user: "ada",
      id: "pk-1",
    });
    return { env, ...(await call(env, "get", get())) };
  }

  it("Use another device or the browser hands it back", async () => {
    const { env, answer, id } = await pending();
    await env.background.passkeys.onWindow(w(id, { kind: "browser" }));
    expect(await answer).toEqual({ fallback: true });
    expect(
      await env.background.passkeys.onWindow(w(id, { kind: "prompt" })),
    ).toEqual({
      state: "gone",
    });
  });

  it("closing the window is a cancel", async () => {
    const { env, answer } = await pending();
    await env.background.passkeys.onWindowClosed(1);
    expect(await answer).toEqual({ error: "NotAllowedError" });
  });

  it("the request's timeout ends it", async () => {
    vi.useFakeTimers();
    const env = await setup();
    env.core.passkeysMade.push({
      vaultId: "vault-1",
      host: "example.com",
      user: "ada",
      id: "pk-1",
    });
    const answer = env.background.passkeys.onCall(
      from(),
      "get",
      get({ timeout: 45_000 }),
    );
    await vi.advanceTimersByTimeAsync(44_000);
    let settled = false;
    void answer.then(() => (settled = true));
    await vi.advanceTimersByTimeAsync(0);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(await answer).toEqual({ error: "NotAllowedError" });
  });

  it("a window of another request id gets nothing", async () => {
    const { env } = await pending();
    expect(
      await env.background.passkeys.onWindow(w("other", { kind: "create" })),
    ).toEqual({
      state: "gone",
    });
  });
});

describe("the switch", () => {
  it("needs site access and registers the scripts", async () => {
    const env = await setup({ enabled: false });
    expect(await env.background.handle({ kind: "setPasskeys", on: true })).toBe(
      false,
    );
    expect(env.pages.passkeyScripts).toBe(false);
    env.pages.siteAccess = true;
    expect(await env.background.handle({ kind: "setPasskeys", on: true })).toBe(
      true,
    );
    expect(env.pages.passkeyScripts).toBe(true);
    expect(await env.background.handle({ kind: "passkeys" })).toBe(true);
    expect(
      await env.background.handle({ kind: "setPasskeys", on: false }),
    ).toBe(false);
    expect(env.pages.passkeyScripts).toBe(false);
  });
});

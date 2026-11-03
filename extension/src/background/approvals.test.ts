/**
 * Approving a new device (D46) in the background: asking while signed out
 * (the Secret Key never reaches the popup), and approving from the manage
 * page. The relay and the core are fakes; the exchange is tested in Rust.
 */
import { describe, expect, it } from "vitest";
import type { Request } from "../shared/messages.ts";
import {
  FakeCore,
  PASSWORD,
  SECRET_KEY,
  fakeFunctions,
  fakePlatform,
} from "./fakes.ts";
import { Background, ServiceError } from "./service.ts";

async function started() {
  const platform = fakePlatform();
  const fns = fakeFunctions();
  const background = await Background.start(fns, platform);
  const core = (): FakeCore => {
    const last = fns.cores.at(-1);
    if (!last) throw new Error("no core");
    return last;
  };
  return { background, platform, fns, core };
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

describe("asking a signed-in device", () => {
  it("shows the code, keeps the Secret Key, then signs in with the password", async () => {
    const { background, platform, fns, core } = await started();
    fns.nextAsk = [
      { kind: "wait" },
      { kind: "send" },
      { kind: "code", code: "123 456" },
      { kind: "wait" },
      { kind: "approved", secretKey: SECRET_KEY },
    ];
    expect(
      await background.handle({
        kind: "askApproval",
        email: " ada@example.com ",
      }),
    ).toEqual({ state: "waiting", code: null });
    // E accepted: N reveals its key and shows the code.
    expect(await background.handle({ kind: "askApprovalPoll" })).toEqual({
      state: "code",
      code: "123 456",
    });
    expect(await background.handle({ kind: "askApprovalPoll" })).toEqual({
      state: "code",
      code: "123 456",
    });
    const approved = await background.handle({ kind: "askApprovalPoll" });
    expect(approved).toEqual({ state: "approved", code: null });
    expect(JSON.stringify(approved)).not.toContain(SECRET_KEY);
    expect(platform.requests.map((r) => r.url)).toEqual(
      expect.arrayContaining([
        expect.stringContaining("/v1/approvals"),
        expect.stringContaining("/v1/approvals/ask-1/reveal"),
      ]),
    );

    await background.handle({ kind: "signInApproved", password: PASSWORD });
    expect((await background.handle({ kind: "status" })).state).toBe(
      "unlocked",
    );
    expect(await platform.disk.get("secretKey")).toBe(SECRET_KEY);
    expect(core().isUnlocked()).toBe(true);
    // Used once.
    expect(
      await errorOf(
        background.handle({ kind: "signInApproved", password: PASSWORD }),
      ),
    ).toBe("approval_ended");
  });

  it("ends when denied, and gives up on the server when cancelled", async () => {
    const { background, fns } = await started();
    fns.nextAsk = [{ kind: "wait" }, { kind: "ended" }];
    await background.handle({ kind: "askApproval", email: "ada@example.com" });
    expect(await background.handle({ kind: "askApprovalPoll" })).toEqual({
      state: "ended",
      code: null,
    });
    expect(await background.handle({ kind: "askApprovalPoll" })).toEqual({
      state: "ended",
      code: null,
    });

    fns.nextAsk = [{ kind: "wait" }];
    await background.handle({ kind: "askApproval", email: "ada@example.com" });
    await background.handle({ kind: "askApprovalCancel" });
    expect(fns.asks.at(-1)?.cancelled).toBe(true);
    expect(
      await errorOf(background.handle({ kind: "askApproval", email: "" })),
    ).toBe("invalid_email");
  });

  it("says so when the server can't be reached", async () => {
    const { background, platform, fns } = await started();
    fns.nextAsk = [{ kind: "wait" }];
    platform.network.down = true;
    expect(
      await errorOf(
        background.handle({ kind: "askApproval", email: "ada@example.com" }),
      ),
    ).toBe("network");
  });
});

describe("approving from a signed-in device", () => {
  async function signedUp() {
    const env = await started();
    await env.background.handle({
      kind: "signUp",
      email: "ada@example.com",
      password: PASSWORD,
    });
    env.core().approvals_ = [
      {
        id: "a1",
        platform: "ios",
        created_at: 1_000,
        state: "pending",
        code: null,
      },
    ];
    return env;
  }

  it("lists, accepts, shows the code and sends the Secret Key", async () => {
    const { background, core, platform } = await signedUp();
    const list = await background.handle({ kind: "approvals" });
    expect(list).toEqual([
      {
        id: "a1",
        platform: "ios",
        createdAt: 1_000,
        state: "pending",
        code: null,
      },
    ]);
    expect(platform.pages.globalBadge).toBe("1");
    const accepted = await background.handle({
      kind: "acceptApproval",
      id: "a1",
    });
    expect(accepted[0]?.code).toBe("654 321");
    expect(
      await background.handle({ kind: "completeApproval", id: "a1" }),
    ).toEqual([]);
    expect(core().completedWith).toBe(SECRET_KEY);
    expect(platform.pages.globalBadge).toBe("");
  });

  it("denies, refuses an unknown request, and counts requests at sync", async () => {
    const { background, core, platform } = await signedUp();
    expect(
      await errorOf(background.handle({ kind: "acceptApproval", id: "gone" })),
    ).toBe("unknown_approval");
    expect(await background.handle({ kind: "denyApproval", id: "a1" })).toEqual(
      [],
    );
    core().approvals_ = [
      {
        id: "a2",
        platform: "macos",
        created_at: 2_000,
        state: "pending",
        code: null,
      },
    ];
    const status = await background.handle({ kind: "sync" } satisfies Request);
    expect(status.state === "unlocked" && status.approvals).toBe(1);
    expect(platform.pages.globalBadge).toBe("1");
    await background.handle({ kind: "lock" });
    expect(await errorOf(background.handle({ kind: "approvals" }))).toBe(
      "locked",
    );
  });
});

import { describe, expect, it } from "vitest";
import { HOSTED_SERVER } from "../shared/config.ts";
import { fromBase64 } from "../shared/bytes.ts";
import type { Request } from "../shared/messages.ts";
import {
  FakeCore,
  ITEM_PASSWORD,
  PASSWORD,
  SECRET_KEY,
  TOTP_SECRET,
  done,
  fakeFunctions,
  fakePlatform,
  wait,
} from "./fakes.ts";
import type { FakePlatform } from "./fakes.ts";
import { localDate } from "./kit.ts";
import { Background, RULES_PATH, ServiceError } from "./service.ts";

const MINUTE = 60_000;

async function started(platform: FakePlatform = fakePlatform()) {
  const fns = fakeFunctions();
  const background = await Background.start(fns, platform);
  const core = (): FakeCore => {
    const last = fns.cores.at(-1);
    if (!last) throw new Error("no core");
    return last;
  };
  return { background, platform, fns, core };
}

async function signedUp() {
  const env = await started();
  await env.background.handle({
    kind: "signUp",
    email: "ada@example.com",
    password: PASSWORD,
  });
  env.core().addLogin("vault-1", {
    id: "item-1",
    type: "login",
    title: "Example",
    username: "ada",
    password: ITEM_PASSWORD,
    totp: TOTP_SECRET,
    urls: [{ url: "https://example.com/login" }],
  });
  // As if the item came in with a sync, which saves the cache.
  await env.background.handle({ kind: "sync" });
  return env;
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

describe("sign-up", () => {
  it("creates the account and its first vault, and stores only what D34 allows", async () => {
    const { background, platform, core } = await started();
    const reply = await background.handle({
      kind: "signUp",
      email: "ada@example.com",
      password: PASSWORD,
    });

    expect(reply).toEqual({ secretKey: SECRET_KEY });
    expect(await background.status()).toMatchObject({
      state: "unlocked",
      email: "ada@example.com",
    });
    expect(core().vaultIds()).toEqual(["vault-1"]);

    const disk = Object.fromEntries(platform.disk.values);
    expect(disk.secretKey).toBe(SECRET_KEY);
    expect(disk.email).toBe("ada@example.com");
    expect(disk.deviceId).toBe("device-1");
    // Tokens only sealed; never the password.
    expect(disk.sealedTokens).toBeDefined();
    const everything =
      JSON.stringify(disk) + new TextDecoder().decode(platform.disk.snapshot);
    expect(everything).not.toContain("refresh-1");
    expect(everything).not.toContain("access-1");
    expect(everything).not.toContain(PASSWORD);
    expect(
      new TextDecoder().decode(fromBase64(disk.sealedTokens ?? "")),
    ).toMatch(/^sealed:/);

    expect(platform.sessionBytes.value).toBeDefined();
    expect(platform.alarmsAt.get("autolock")).toBe(
      platform.clock.now + 15 * MINUTE,
    );
    expect(platform.disk.snapshot).toBeDefined();
  });

  it("reports the core's error code", async () => {
    const { background } = await started();
    expect(
      await errorOf(
        background.handle({
          kind: "signUp",
          email: "taken@example.com",
          password: PASSWORD,
        }),
      ),
    ).toBe("account_exists");
    expect(await background.status()).toEqual({ state: "signedOut" });
  });

  it("reports an unreachable server as network", async () => {
    const { background, platform } = await started();
    platform.network.down = true;
    expect(
      await errorOf(
        background.handle({
          kind: "signUp",
          email: "ada@example.com",
          password: PASSWORD,
        }),
      ),
    ).toBe("network");
  });

  it("keeps the account when the first vault can't be made yet", async () => {
    const { background, platform, core } = await started();
    const signUp = background.handle({
      kind: "signUp",
      email: "ada@example.com",
      password: PASSWORD,
    });
    // Fail the ensure-vault request only.
    const fake = core();
    const realEnsure = fake.startEnsureVault.bind(fake);
    fake.startEnsureVault = () => {
      const step = realEnsure();
      fake.next = () => wait(30_000);
      return step;
    };
    await signUp;
    expect(await background.status()).toMatchObject({
      state: "unlocked",
      syncError: "offline",
    });
    expect(platform.alarmsAt.get("sync-retry")).toBe(
      platform.clock.now + 30_000,
    );
  });
});

describe("sign-in", () => {
  it("signs in with email, password and Secret Key", async () => {
    const { background, platform } = await started();
    await background.handle({
      kind: "signIn",
      email: "ada@example.com",
      password: PASSWORD,
      secretKey: SECRET_KEY,
    });
    expect(await background.status()).toMatchObject({ state: "unlocked" });
    expect(platform.disk.values.get("secretKey")).toBe(SECRET_KEY);
  });

  it("refuses wrong credentials", async () => {
    const { background, platform } = await started();
    const code = await errorOf(
      background.handle({
        kind: "signIn",
        email: "ada@example.com",
        password: "nope",
        secretKey: SECRET_KEY,
      }),
    );
    expect(code).toBe("invalid_credentials");
    expect(platform.disk.values.has("secretKey")).toBe(false);
  });

  it("sends every request without cookies, caching or redirects", async () => {
    const { background, platform } = await started();
    await background.handle({
      kind: "signIn",
      email: "ada@example.com",
      password: PASSWORD,
      secretKey: SECRET_KEY,
    });
    expect(platform.requests.length).toBeGreaterThan(0);
    for (const { url, init } of platform.requests) {
      expect(url.startsWith(HOSTED_SERVER)).toBe(true);
      expect(init).toMatchObject({
        credentials: "omit",
        cache: "no-store",
        redirect: "error",
      });
    }
  });
});

describe("the popup never receives secrets", () => {
  it("lists items without passwords or one-time code secrets", async () => {
    const { background } = await signedUp();
    const items = await background.handle({ kind: "items", query: "" });
    expect(items).toEqual([
      {
        vaultId: "vault-1",
        itemId: "item-1",
        title: "Example",
        username: "ada",
        url: "https://example.com/login",
        hasPassword: true,
        hasCode: true,
      },
    ]);
    const text = JSON.stringify(items);
    expect(text).not.toContain(ITEM_PASSWORD);
    expect(text).not.toContain(TOTP_SECRET);
  });

  it("answers no request with a key, token or item password", async () => {
    const { background } = await signedUp();
    const requests: Request[] = [
      { kind: "status" },
      { kind: "items", query: "ex" },
      { kind: "copy", vaultId: "vault-1", itemId: "item-1", field: "password" },
      { kind: "copy", vaultId: "vault-1", itemId: "item-1", field: "code" },
      { kind: "copySecretKey" },
      { kind: "sync" },
      { kind: "settings" },
      { kind: "activity" },
    ];
    for (const request of requests) {
      const text = JSON.stringify(await background.handle(request));
      for (const secret of [
        ITEM_PASSWORD,
        TOTP_SECRET,
        SECRET_KEY,
        "287082",
        "refresh-1",
        "access-1",
        PASSWORD,
      ]) {
        expect(text, request.kind).not.toContain(secret);
      }
    }
  });

  it("searches title, username and address", async () => {
    const { background } = await signedUp();
    expect(
      await background.handle({ kind: "items", query: "EXAMPLE.COM" }),
    ).toHaveLength(1);
    expect(
      await background.handle({ kind: "items", query: "ada" }),
    ).toHaveLength(1);
    expect(
      await background.handle({ kind: "items", query: "nothing" }),
    ).toHaveLength(0);
  });

  it("leaves out trashed items", async () => {
    const { background, core } = await signedUp();
    core().addLogin("vault-1", {
      id: "item-2",
      type: "login",
      title: "Old",
      trashed_at: 5,
    });
    expect(await background.handle({ kind: "items", query: "" })).toHaveLength(
      1,
    );
  });
});

describe("clipboard", () => {
  it("copies through the clipboard and clears it 30 seconds later", async () => {
    const { background, platform } = await signedUp();
    await background.handle({
      kind: "copy",
      vaultId: "vault-1",
      itemId: "item-1",
      field: "password",
    });
    expect(platform.clipboardText.value).toBe(ITEM_PASSWORD);
    expect(platform.alarmsAt.get("clipboard")).toBe(
      platform.clock.now + 30_000,
    );

    await background.onAlarm("clipboard");
    expect(platform.clipboardText.value).toBe("");
  });

  it("copies the current one-time code, not its secret", async () => {
    const { background, platform } = await signedUp();
    await background.handle({
      kind: "copy",
      vaultId: "vault-1",
      itemId: "item-1",
      field: "code",
    });
    expect(platform.clipboardText.value).toBe("287082");
  });

  it("refuses to copy an empty field or an unknown item", async () => {
    const { background, core } = await signedUp();
    core().addLogin("vault-1", {
      id: "item-2",
      type: "login",
      title: "No password",
    });
    expect(
      await errorOf(
        background.handle({
          kind: "copy",
          vaultId: "vault-1",
          itemId: "item-2",
          field: "password",
        }),
      ),
    ).toBe("empty");
    expect(
      await errorOf(
        background.handle({
          kind: "copy",
          vaultId: "vault-1",
          itemId: "nope",
          field: "username",
        }),
      ),
    ).toBe("unknown_item");
  });

  it("copies the Secret Key with the same 30-second clear", async () => {
    const { background, platform } = await signedUp();
    await background.handle({ kind: "copySecretKey" });
    expect(platform.clipboardText.value).toBe(SECRET_KEY);
    expect(platform.alarmsAt.get("clipboard")).toBe(
      platform.clock.now + 30_000,
    );
  });

  it("makes the Emergency Kit PDF for the signed-in account (D42)", async () => {
    const { background, platform, core } = await signedUp();
    const { pdf } = await background.handle({ kind: "emergencyKit" });
    // The hosted server is not printed, and en-GB prints on A4.
    expect(new TextDecoder().decode(fromBase64(pdf))).toBe(
      `%PDF-fake ada@example.com ${SECRET_KEY} hosted ${String(core().accountId())} ${localDate(platform.clock.now)} a4`,
    );
  });

  it("has no Emergency Kit when signed out", async () => {
    const { background } = await started();
    expect(await errorOf(background.handle({ kind: "emergencyKit" }))).toBe(
      "signed_out",
    );
  });
});

describe("auto-lock", () => {
  it("restarts the timer on activity", async () => {
    const { background, platform } = await signedUp();
    platform.clock.now += 10 * MINUTE;
    await background.handle({ kind: "activity" });
    expect(platform.alarmsAt.get("autolock")).toBe(
      platform.clock.now + 15 * MINUTE,
    );
  });

  it("locks when the alarm fires and forgets the session", async () => {
    const { background, platform } = await signedUp();
    await background.onAlarm("autolock");
    expect(await background.status()).toEqual({
      state: "locked",
      email: "ada@example.com",
    });
    expect(platform.sessionBytes.value).toBeUndefined();
    expect(platform.alarmsAt.has("autolock")).toBe(false);
    expect(await errorOf(background.handle({ kind: "items", query: "" }))).toBe(
      "locked",
    );
  });

  it("locks at once when the computer locks", async () => {
    const { background, platform } = await signedUp();
    await background.onScreenLocked();
    expect((await background.status()).state).toBe("locked");
    expect(platform.sessionBytes.value).toBeUndefined();
  });

  it("uses the chosen timer", async () => {
    const { background, platform } = await signedUp();
    await background.handle({
      kind: "setSettings",
      settings: { serverUrl: HOSTED_SERVER, autoLockMinutes: 60 },
    });
    expect(platform.alarmsAt.get("autolock")).toBe(
      platform.clock.now + 60 * MINUTE,
    );
  });

  it("unlocks with the master password and the stored Secret Key", async () => {
    const { background, platform } = await signedUp();
    await background.handle({ kind: "lock" });
    expect(
      await errorOf(background.handle({ kind: "unlock", password: "wrong" })),
    ).toBe("wrong_password");
    await background.handle({ kind: "unlock", password: PASSWORD });
    expect((await background.status()).state).toBe("unlocked");
    expect(platform.sessionBytes.value).toBeDefined();
  });
});

describe("worker restart", () => {
  it("restores the unlocked session from session storage", async () => {
    const { platform } = await signedUp();
    const restarted = await started(platform);
    expect(await restarted.background.status()).toMatchObject({
      state: "unlocked",
      email: "ada@example.com",
    });
    expect(
      await restarted.background.handle({ kind: "items", query: "" }),
    ).toHaveLength(1);
  });

  it("starts locked after the browser closed (session storage is empty)", async () => {
    const { platform } = await signedUp();
    platform.sessionBytes.value = undefined;
    const restarted = await started(platform);
    expect((await restarted.background.status()).state).toBe("locked");
  });

  it("drops a session that no longer opens", async () => {
    const { platform } = await signedUp();
    platform.sessionBytes.value = new Uint8Array([9]);
    const restarted = await started(platform);
    expect((await restarted.background.status()).state).toBe("locked");
    expect(platform.sessionBytes.value).toBeUndefined();
  });

  it("starts empty from a damaged snapshot", async () => {
    const platform = fakePlatform();
    platform.disk.snapshot = new Uint8Array([0xde, 0xad]);
    const { background } = await started(platform);
    expect(await background.status()).toEqual({ state: "signedOut" });
  });
});

describe("sync", () => {
  it("records the time of a good sync", async () => {
    const { background, platform } = await signedUp();
    platform.clock.now += 5000;
    expect(await background.handle({ kind: "sync" })).toMatchObject({
      syncedAt: platform.clock.now,
      syncError: null,
    });
  });

  it("retries later when the core asks to wait", async () => {
    const { background, platform, core } = await signedUp();
    core().syncResult = () => wait(5000);
    expect(await background.handle({ kind: "sync" })).toMatchObject({
      syncError: "offline",
    });
    expect(platform.alarmsAt.get("sync-retry")).toBe(platform.clock.now + 5000);
    core().syncResult = () => done("synced");
    await background.onAlarm("sync-retry");
    expect(await background.status()).toMatchObject({ syncError: null });
  });

  it("shares one run between concurrent requests", async () => {
    const { background, platform } = await signedUp();
    const syncs = () =>
      platform.requests.filter((r) => r.url.endsWith("/v1/sync")).length;
    const before = syncs();
    await Promise.all([
      background.sync(),
      background.sync(),
      background.handle({ kind: "sync" }),
    ]);
    expect(syncs() - before).toBe(1);
  });

  it("forgets the account on this device when it was revoked", async () => {
    const { background, platform, core } = await signedUp();
    core().syncResult = () => {
      core().account = undefined;
      core().unlocked = false;
      return done("device_revoked");
    };
    await background.handle({ kind: "sync" });
    expect(await background.status()).toEqual({ state: "signedOut" });
    expect(platform.disk.values.has("secretKey")).toBe(false);
    expect(platform.disk.values.has("sealedTokens")).toBe(false);
    expect(platform.sessionBytes.value).toBeUndefined();
  });

  it("reports a server error without throwing", async () => {
    const { background, platform } = await signedUp();
    platform.network.status = 503;
    expect(await background.handle({ kind: "sync" })).toMatchObject({
      state: "unlocked",
      syncError: "server",
    });
  });

  it("seals refreshed tokens again", async () => {
    const { background, platform, core } = await signedUp();
    platform.disk.values.delete("sealedTokens");
    core().syncResult = () => {
      core().tokens = ["access-2", "refresh-2"];
      return done("synced");
    };
    await background.handle({ kind: "sync" });
    expect(platform.disk.values.get("sealedTokens")).toBeDefined();
  });
});

describe("settings", () => {
  it("accepts an https server or a local one, and refuses others", async () => {
    const { background } = await started();
    for (const serverUrl of [
      "https://vault.example.org",
      "https://vault.example.org/",
      "http://localhost:8080",
    ]) {
      const saved = await background.handle({
        kind: "setSettings",
        settings: { serverUrl, autoLockMinutes: 15 },
      });
      expect(saved.serverUrl).toBe(serverUrl.replace(/\/$/, ""));
    }
    for (const serverUrl of [
      "http://vault.example.org",
      "ftp://x",
      "https://a.example/path",
      "javascript:alert(1)",
    ]) {
      expect(
        await errorOf(
          background.handle({
            kind: "setSettings",
            settings: { serverUrl, autoLockMinutes: 15 },
          }),
        ),
      ).toBe("invalid_server");
    }
  });

  it("offers no 'never' for auto-lock", async () => {
    const { background } = await started();
    for (const autoLockMinutes of [0, 2, 10_000]) {
      expect(
        await errorOf(
          background.handle({
            kind: "setSettings",
            settings: { serverUrl: HOSTED_SERVER, autoLockMinutes },
          }),
        ),
      ).toBe("invalid_auto_lock");
    }
  });

  it("keeps the server while an account is signed in", async () => {
    const { background } = await signedUp();
    expect(
      await errorOf(
        background.handle({
          kind: "setSettings",
          settings: { serverUrl: "https://other.example", autoLockMinutes: 15 },
        }),
      ),
    ).toBe("signed_in");
  });

  it("talks to the self-hosted server once set", async () => {
    const { background, platform } = await started();
    await background.handle({
      kind: "setSettings",
      settings: { serverUrl: "https://vault.example.org", autoLockMinutes: 5 },
    });
    await background.handle({
      kind: "signIn",
      email: "ada@example.com",
      password: PASSWORD,
      secretKey: SECRET_KEY,
    });
    expect(
      platform.requests.every((r) =>
        r.url.startsWith("https://vault.example.org/"),
      ),
    ).toBe(true);
    const restarted = await started(platform);
    expect(await restarted.background.handle({ kind: "settings" })).toEqual({
      serverUrl: "https://vault.example.org",
      autoLockMinutes: 5,
    });
  });

  it("signs out: forgets the account, keeps the device id", async () => {
    const { background, platform } = await signedUp();
    await background.handle({ kind: "signOut" });
    expect(await background.status()).toEqual({ state: "signedOut" });
    expect([...platform.disk.values.keys()].sort()).toEqual(["deviceId"]);
    expect(platform.sessionBytes.value).toBeUndefined();
  });
});

describe("rules download", () => {
  const rulesRequests = (platform: FakePlatform) =>
    platform.requests.filter((r) => r.url.endsWith(RULES_PATH));

  it("installs a newer rules file and schedules the next download in about a day", async () => {
    const { background, platform, core } = await started();
    platform.network.body = new Uint8Array([7, 1, 2]);
    platform.network.etag = '"v7"';
    await background.updateRules();
    expect(core().rulesVersions).toEqual([{ version: 7, minVersion: 0 }]);
    expect(platform.disk.values.get("rulesVersion")).toBe("7");
    expect(platform.disk.values.get("rulesEtag")).toBe('"v7"');
    const next = (platform.alarmsAt.get("rules") ?? 0) - platform.clock.now;
    expect(next).toBeGreaterThanOrEqual(22 * 60 * MINUTE);
    expect(next).toBeLessThanOrEqual(26 * 60 * MINUTE);
  });

  it("refuses a rollback and keeps the current rules", async () => {
    const { background, platform, core } = await started();
    platform.network.body = new Uint8Array([7]);
    await background.updateRules();
    platform.network.body = new Uint8Array([5]);
    await background.updateRules();
    expect(core().rulesVersions).toEqual([{ version: 7, minVersion: 0 }]);
    expect(platform.disk.values.get("rulesVersion")).toBe("7");
  });

  it("asks with the stored ETag and keeps the rules on 304", async () => {
    const { background, platform } = await started();
    platform.network.body = new Uint8Array([7]);
    platform.network.etag = '"v7"';
    await background.updateRules();
    platform.network.status = 304;
    await background.updateRules();
    const last = rulesRequests(platform).at(-1);
    expect(last?.init.headers).toEqual({ "if-none-match": '"v7"' });
    expect(platform.disk.values.get("rulesVersion")).toBe("7");
  });

  it("keeps going when offline or refused, and tries again later", async () => {
    const { background, platform } = await started();
    platform.network.down = true;
    await background.updateRules();
    expect(platform.alarmsAt.has("rules")).toBe(true);
    platform.network.down = false;
    platform.network.body = new Uint8Array([0xff]);
    await background.updateRules();
    expect(platform.disk.values.has("rulesVersion")).toBe(false);
  });

  it("gives the core the last accepted file again after a restart", async () => {
    const { background, platform } = await started();
    platform.network.body = new Uint8Array([7]);
    await background.updateRules();
    const restarted = await started(platform);
    expect(restarted.core().rulesVersions).toEqual([
      { version: 7, minVersion: 7 },
    ]);
  });

  it("fetches from the hosted server even when self-hosting", async () => {
    const { background, platform } = await started();
    await background.handle({
      kind: "setSettings",
      settings: { serverUrl: "https://vault.example.org", autoLockMinutes: 15 },
    });
    await background.updateRules();
    expect(rulesRequests(platform)[0]?.url).toBe(HOSTED_SERVER + RULES_PATH);
  });
});

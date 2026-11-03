/**
 * The manage page's requests (D44): devices, the health report with its leak
 * check, and export. The core is the fake one; the real calls are tested in
 * Rust and through WASM.
 */
import { describe, expect, it } from "vitest";
import { fromBase64 } from "../shared/bytes.ts";
import {
  EXPORT_PASSWORD,
  FakeCore,
  ITEM_PASSWORD,
  PASSWORD,
  fakeFunctions,
  fakePlatform,
} from "./fakes.ts";
import { LEAK_CHECK_ORIGIN } from "./manage.ts";
import { Background, ServiceError } from "./service.ts";

async function signedUp() {
  const platform = fakePlatform();
  const fns = fakeFunctions();
  const background = await Background.start(fns, platform);
  await background.handle({
    kind: "signUp",
    email: "ada@example.com",
    password: PASSWORD,
  });
  const core = (): FakeCore => {
    const last = fns.cores.at(-1);
    if (!last) throw new Error("no core");
    return last;
  };
  for (const [id, title, url] of [
    ["item-1", "Bank", "https://bank.example/login"],
    ["item-2", "Shop", "https://shop.example"],
    ["item-3", "Forum", ""],
  ] as const) {
    core().addLogin("vault-1", {
      id,
      type: "login",
      title,
      username: "ada",
      password: ITEM_PASSWORD,
      urls: url === "" ? [] : [{ url }],
    });
  }
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

describe("devices", () => {
  it("lists, renames and signs out another device", async () => {
    const { background } = await signedUp();
    const devices = await background.handle({ kind: "devices" });
    expect(devices.map((d) => [d.name, d.platform, d.current])).toEqual([
      ["Browser extension", "extension", true],
      ["iPhone", "ios", false],
    ]);
    expect(devices[1]).toMatchObject({
      createdAt: 1_500,
      lastSeenAt: 1_800,
      revokedAt: null,
    });

    const renamed = await background.handle({
      kind: "renameDevice",
      deviceId: "device-2",
      name: "  Work iPhone ",
    });
    expect(renamed[1]?.name).toBe("Work iPhone");
    const revoked = await background.handle({
      kind: "revokeDevice",
      deviceId: "device-2",
    });
    expect(revoked[1]?.revokedAt).toBe(3_000);
  });

  it("never signs out this device, and needs a name and an unlocked vault", async () => {
    const { background } = await signedUp();
    expect(
      await errorOf(
        background.handle({ kind: "revokeDevice", deviceId: "device-1" }),
      ),
    ).toBe("this_device");
    expect(
      await errorOf(
        background.handle({
          kind: "renameDevice",
          deviceId: "device-2",
          name: "   ",
        }),
      ),
    ).toBe("invalid_device_name");
    expect(
      await errorOf(
        background.handle({
          kind: "renameDevice",
          deviceId: "gone",
          name: "x",
        }),
      ),
    ).toBe("unknown_device");
    await background.handle({ kind: "lock" });
    expect(await errorOf(background.handle({ kind: "devices" }))).toBe(
      "locked",
    );
  });
});

describe("the health report", () => {
  it("names each item and leaves out ones that are gone", async () => {
    const { background, core } = await signedUp();
    core().health_ = {
      weak: [
        { item_id: "item-3", score: 1 },
        { item_id: "gone", score: 0 },
      ],
      reused: [
        ["item-1", "item-2"],
        ["item-2", "gone"],
      ],
    };
    const health = await background.handle({ kind: "health" });
    expect(health.weak.map((i) => i.title)).toEqual(["Forum"]);
    expect(health.reused.map((g) => g.map((i) => i.title))).toEqual([
      ["Bank", "Shop"],
    ]);
    expect(health.weak[0]).toMatchObject({
      vaultId: "vault-1",
      itemId: "item-3",
      url: "",
    });
    expect(health.leaked).toBeNull();
    expect(health.leakCheck).toBe(true);
  });

  it("checks for leaks only with the user's permission, straight to HIBP", async () => {
    const { background, platform, core } = await signedUp();
    core().breached = [{ item_id: "item-1", count: 42 }];
    expect(await errorOf(background.handle({ kind: "checkLeaks" }))).toBe(
      "no_leak_access",
    );
    const toHibp = () =>
      platform.requests.filter((r) => r.url.includes("pwnedpasswords"));
    expect(toHibp()).toHaveLength(0);

    platform.pages.hostAccess.add(LEAK_CHECK_ORIGIN);
    platform.network.body = new TextEncoder().encode("0123:0\r\n");
    const health = await background.handle({ kind: "checkLeaks" });
    expect(health.leaked?.map((i) => [i.title, i.count])).toEqual([
      ["Bank", 42],
    ]);
    expect(health.leaked?.[0]?.url).toBe("https://bank.example/login");
    const leakRequests = toHibp();
    expect(leakRequests.map((r) => r.url)).toEqual([
      "https://api.pwnedpasswords.com/range/ABCDE",
      "https://api.pwnedpasswords.com/range/12345",
    ]);
    for (const { init } of leakRequests) {
      expect(new Headers(init.headers).get("add-padding")).toBe("true");
      expect(init.credentials).toBe("omit");
      expect(init.cache).toBe("no-store");
      expect(new Headers(init.headers).get("authorization")).toBeNull();
    }
    expect(core().breachBodies).toHaveLength(2);

    // Kept until the vault locks.
    expect((await background.handle({ kind: "health" })).leaked).toHaveLength(
      1,
    );
    await background.handle({ kind: "lock" });
    await background.handle({ kind: "unlock", password: PASSWORD });
    expect((await background.handle({ kind: "health" })).leaked).toBeNull();
  });

  it("reports a failed check and can be turned off", async () => {
    const { background, platform } = await signedUp();
    platform.pages.hostAccess.add(LEAK_CHECK_ORIGIN);
    platform.network.status = 503;
    expect(await errorOf(background.handle({ kind: "checkLeaks" }))).toBe(
      "breach_check",
    );
    platform.network.down = true;
    expect(await errorOf(background.handle({ kind: "checkLeaks" }))).toBe(
      "network",
    );

    expect(await background.handle({ kind: "setLeakCheck", on: false })).toBe(
      false,
    );
    expect((await background.handle({ kind: "health" })).leakCheck).toBe(false);
    expect(await errorOf(background.handle({ kind: "checkLeaks" }))).toBe(
      "leak_check_off",
    );
  });
});

describe("export", () => {
  it("asks for the master password every time and returns the file once", async () => {
    const { background, core, platform } = await signedUp();
    platform.clock.now = Date.UTC(2026, 10, 3, 12);
    expect(
      await errorOf(
        background.handle({
          kind: "exportVault",
          masterPassword: "wrong",
          format: "json",
          exportPassword: "",
        }),
      ),
    ).toBe("wrong_password");

    const json = await background.handle({
      kind: "exportVault",
      masterPassword: PASSWORD,
      format: "json",
      exportPassword: "",
    });
    expect(new TextDecoder().decode(fromBase64(json.file))).toBe("export:json");
    expect(json.name).toBe("PWM export 2026-11-03.json");
    expect(core().exportGrant).toBe(false);

    const pwmx = await background.handle({
      kind: "exportVault",
      masterPassword: PASSWORD,
      format: "pwmx",
      exportPassword: EXPORT_PASSWORD,
    });
    expect(new TextDecoder().decode(fromBase64(pwmx.file))).toBe(
      `export:pwmx:${EXPORT_PASSWORD}`,
    );
    expect(pwmx.name.endsWith(".pwmx")).toBe(true);
  });

  it("refuses a weak export password before checking the master password", async () => {
    const { background, core } = await signedUp();
    const before = core().authorizations;
    expect(
      await errorOf(
        background.handle({
          kind: "exportVault",
          masterPassword: PASSWORD,
          format: "pwmx",
          exportPassword: "password",
        }),
      ),
    ).toBe("weak_export_password");
    expect(core().authorizations).toBe(before);
    expect(core().exportGrant).toBe(false);
    expect(
      await errorOf(
        background.handle({
          kind: "exportVault",
          masterPassword: PASSWORD,
          format: "xml" as "json",
          exportPassword: "",
        }),
      ),
    ).toBe("invalid_request");
  });
});

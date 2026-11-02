/**
 * The background over the real WASM core (Node build from `just wasm`):
 * the typings in `pwm-core.d.ts` against the build, and the snapshot round
 * trip through storage (the M3.7 deferral). The protocol is tested in Rust.
 *
 *   pnpm test:wasm   (PWM_WASM_NODE overrides the build path)
 */
import { createRequire } from "node:module";
import path from "node:path";
import { describe, expect, it } from "vitest";
import type * as Wasm from "pwm-core";
import type { Core } from "./core.ts";
import { MemoryDisk, PASSWORD, fakePlatform } from "./fakes.ts";
import { coreFunctions } from "./functions.ts";
import { Background, ServiceError } from "./service.ts";

const file =
  process.env.PWM_WASM_NODE ??
  path.resolve(import.meta.dirname, "../../../target/wasm/node/pwm_wasm.js");
const wasm = createRequire(import.meta.url)(file) as typeof Wasm;
const fns = coreFunctions(wasm);

const CORE_METHODS: (keyof Core)[] = [
  "snapshot",
  "accountId",
  "isUnlocked",
  "startSignup",
  "startLogin",
  "startSync",
  "startEnsureVault",
  "receive",
  "networkFailed",
  "cancel",
  "takeTokens",
  "sealTokens",
  "unlockWithSealedTokens",
  "exportSession",
  "restoreSession",
  "lock",
  "vaultIds",
  "items",
  "currentCode",
  "setAutofillRules",
];

describe("the WASM core", () => {
  it("has every method the background calls", () => {
    const core = fns.newCore() as unknown as Record<string, unknown>;
    expect(
      CORE_METHODS.filter((name) => typeof core[name] !== "function"),
    ).toEqual([]);
  });

  it("gives steps the shape the background reads", () => {
    const step = fns
      .newCore()
      .startSignup(
        "ada@example.com",
        PASSWORD,
        fns.newId(),
        "extension",
        "Browser extension",
      );
    expect(step.kind).toBe("send");
    expect(step.method).toBe("POST");
    expect(step.path).toMatch(/^\/v1\//);
    expect(step.headers.length % 2).toBe(0);
    expect(step.body).toBeInstanceOf(Uint8Array);
  });

  it("round-trips the cache snapshot through storage", async () => {
    const disk = new MemoryDisk();
    const first = await Background.start(fns, fakePlatform(disk));
    await first.handle({ kind: "signOut" });
    const saved = disk.snapshot;
    expect(saved).toBeInstanceOf(Uint8Array);
    if (!saved) return;
    expect(fns.loadCore(saved).snapshot()).toEqual(saved);

    const second = await Background.start(fns, fakePlatform(disk));
    expect(await second.status()).toEqual({ state: "signedOut" });
  });

  it("turns a failed network into the network code", async () => {
    const platform = fakePlatform();
    platform.network.down = true;
    const background = await Background.start(fns, platform);
    const error: unknown = await background
      .handle({ kind: "signUp", email: "ada@example.com", password: PASSWORD })
      .catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ServiceError);
    expect((error as ServiceError).code).toBe("network");
    expect(await background.status()).toEqual({ state: "signedOut" });
  });

  it("generates and scores with the core", async () => {
    const background = await Background.start(fns, fakePlatform());
    const password = await background.handle({
      kind: "generate",
      mode: "password",
    });
    expect(password).toHaveLength(20);
    const passphrase = await background.handle({
      kind: "generate",
      mode: "passphrase",
    });
    expect(passphrase.split("-")).toHaveLength(5);
    expect(
      await background.handle({
        kind: "score",
        password: "password",
        email: "ada@example.com",
      }),
    ).toBeLessThan(2);
    expect(
      await background.handle({
        kind: "score",
        password,
        email: "ada@example.com",
      }),
    ).toBe(4);
  });
});

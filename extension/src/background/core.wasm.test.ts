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
  "startSignOut",
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
  "autofillMatches",
  "classifyFields",
  "autofillRecordStep",
  "autofillNextStep",
  "autofillForgetTab",
  "saveItem",
  "item",
  "setTrashed",
  "deleteItem",
  "purgeTrash",
  "saveOffer",
  "saveLogin",
  "updatePassword",
  "passkeyCreateCheck",
  "passkeyCreate",
  "passkeyCandidates",
  "passkeyGet",
  "autofillReport",
  "startListDevices",
  "startRenameDevice",
  "startRevokeDevice",
  "healthReport",
  "breachCheck",
  "authorizeExport",
  "exportFile",
  "startListApprovals",
  "startAcceptApproval",
  "startCompleteApproval",
  "startDenyApproval",
  "prepareImport",
  "finishImport",
  "cancelImport",
];

describe("the WASM core", () => {
  it("refuses the manage page's calls while locked (D44)", () => {
    const core = fns.newCore();
    const locked = expect.objectContaining({ code: "locked" }) as Error;
    expect(() => core.healthReport()).toThrow(locked);
    expect(() => core.breachCheck()).toThrow(locked);
    expect(() => core.startListDevices()).toThrow(locked);
    expect(() => core.exportFile("json", "", Date.now())).toThrow(locked);
    expect(() => core.startListApprovals()).toThrow(locked);
    // Its tokens are sealed while locked (D62).
    expect(() => core.startSignOut()).toThrow(locked);
    expect(() =>
      core.prepareImport(
        "",
        "csv",
        new Uint8Array(),
        "",
        new Uint8Array(),
        "",
        Date.now(),
      ),
    ).toThrow(locked);
    expect(() => core.item(fns.newId(), fns.newId())).toThrow(locked);
    expect(() => {
      core.setTrashed(fns.newId(), fns.newId(), true, Date.now());
    }).toThrow(locked);
    expect(() => core.purgeTrash(Date.now())).toThrow(locked);
    // The one-time code setup (D23): a bare secret becomes a URI.
    expect(fns.normalizeTotp("JBSWY3DPEHPK3PXP", "Mail")).toMatch(
      /^otpauth:\/\/totp\/.*secret=JBSWY3DPEHPK3PXP/,
    );
    expect(() => fns.normalizeTotp("https://example.com", "Mail")).toThrow(
      expect.objectContaining({ code: "invalid_totp" }) as Error,
    );
    expect(fns.breachRangeUrl("ABCDE")).toBe(
      "https://api.pwnedpasswords.com/range/ABCDE",
    );
  });

  it("has every method the background calls", () => {
    const core = fns.newCore() as unknown as Record<string, unknown>;
    expect(
      CORE_METHODS.filter((name) => typeof core[name] !== "function"),
    ).toEqual([]);
  });

  it("classifies fields in the shape the fill plan reads", () => {
    const core = fns.newCore();
    const fields = [
      {
        index: 0,
        tag: "input",
        type: "email",
        name: "email",
        id: "email",
        autocomplete: "username",
        label: "Email",
        placeholder: "",
        aria_label: "",
        inputmode: "",
        max_length: null,
        visible: true,
        form: 0,
      },
      {
        index: 1,
        tag: "input",
        type: "password",
        name: "password",
        id: "password",
        autocomplete: "current-password",
        label: "Password",
        placeholder: "",
        aria_label: "",
        inputmode: "",
        max_length: null,
        visible: true,
        form: 0,
      },
    ];
    const classes = JSON.parse(
      core.classifyFields(JSON.stringify(fields), "https://example.com/login"),
    ) as { class: string; confidence: number }[];
    expect(classes.map((c) => c.class)).toEqual([
      "username",
      "current-password",
    ]);
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

  it("draws the Emergency Kit PDF", () => {
    const pdf = fns.emergencyKit(
      "ada@example.com",
      "A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA",
      undefined,
      "0f8e2f4c-3a1b-4d5e-9f60-7a8b9c0d1e2f",
      "2026-11-02",
      "letter",
    );
    const text = new TextDecoder("latin1").decode(pdf);
    expect(text.startsWith("%PDF-1.4")).toBe(true);
    expect(text).toContain("/MediaBox [0 0 612 792]");
    expect(text).toContain("(A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA)");
    expect(() =>
      fns.emergencyKit("", "A1-NOPE", undefined, "x", "today", "a4"),
    ).toThrow(expect.objectContaining({ code: "invalid_secret_key" }) as Error);
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
    // Five words and one digit, joined by "-". A few list words contain "-"
    // themselves ("t-shirt"), so a phrase can split into up to ten parts.
    const parts = passphrase.split("-").length;
    expect(parts).toBeGreaterThanOrEqual(5);
    expect(parts).toBeLessThanOrEqual(10);
    expect(passphrase.replace(/\D/g, "")).toHaveLength(1);
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

  it("makes a new device's approval request (D46)", () => {
    const ask = fns.newApprovalAsk("ada@example.com");
    const first = ask.firstRequest();
    expect(first?.method).toBe("POST");
    expect(first?.path).toBe("/v1/approvals");
    const body = JSON.parse(new TextDecoder().decode(first?.body)) as {
      email: string;
      commitment: string;
      platform: string;
    };
    expect(body.email).toBe("ada@example.com");
    expect(body.platform).toBe("extension");
    expect(body.commitment).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(ask.firstRequest()).toBeUndefined();
    expect(() => ask.poll()).toThrow(
      expect.objectContaining({ code: "not_running" }) as Error,
    );
  });

  it("detects an import file and reads a CSV's columns (D51)", () => {
    const csv = new TextEncoder().encode(
      "url,username,password,totp,extra,name,grouping,fav\nhttps://b.example,ada,pw,,,Bank,,0\n",
    );
    expect(JSON.parse(fns.detectImport("export.csv", csv) ?? "null")).toEqual({
      format: "lastpass",
      needs_password: false,
    });
    expect(fns.detectImport("photo.png", new Uint8Array([1, 2, 3]))).toBe(
      undefined,
    );
    const generic = new TextEncoder().encode("name,url,username,password\n");
    const columns = JSON.parse(fns.importColumns(generic)) as {
      headers: string[];
      suggested: Record<string, number | null>;
    };
    expect(columns.headers).toEqual(["name", "url", "username", "password"]);
    expect(columns.suggested.password).toBe(3);
    expect(columns.suggested.notes).toBeNull();
  });
});

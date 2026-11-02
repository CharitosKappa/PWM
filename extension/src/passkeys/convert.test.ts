import { describe, expect, it } from "vitest";
import { isPasskeyCall, isPasskeyWindowRequest } from "../shared/passkeys.ts";
import { b64url, buffer, credential, toRequestJson } from "./convert.ts";

const bytes = (...values: number[]) => new Uint8Array(values);

describe("base64url", () => {
  it("round-trips buffers and views, without padding", () => {
    const all = new Uint8Array(Array.from({ length: 256 }, (_, i) => i));
    expect(new Uint8Array(buffer(b64url(all)))).toEqual(all);
    expect(b64url(bytes(0xfb, 0xff))).toBe("-_8");
    const view = new DataView(all.buffer, 10, 3);
    expect(b64url(view)).toBe(b64url(bytes(10, 11, 12)));
  });
});

describe("toRequestJson", () => {
  it("writes creation options in WebAuthn's JSON form", () => {
    const json = JSON.parse(
      toRequestJson({
        challenge: bytes(1, 2, 3),
        rp: { id: "example.com", name: "Example" },
        user: { id: bytes(9), name: "ada", displayName: "Ada" },
        pubKeyCredParams: [{ type: "public-key", alg: -7 }],
        excludeCredentials: [{ type: "public-key", id: bytes(7, 7) }],
        timeout: 60_000,
      }),
    ) as Record<string, unknown>;
    expect(json).toEqual({
      challenge: "AQID",
      rp: { id: "example.com", name: "Example" },
      user: { id: "CQ", name: "ada", displayName: "Ada" },
      pubKeyCredParams: [{ type: "public-key", alg: -7 }],
      timeout: 60_000,
      excludeCredentials: [{ type: "public-key", id: "Bwc" }],
    });
  });

  it("writes request options in WebAuthn's JSON form", () => {
    const json = JSON.parse(
      toRequestJson({
        challenge: bytes(1),
        rpId: "example.com",
        allowCredentials: [{ type: "public-key", id: bytes(2) }],
        userVerification: "required",
      }),
    ) as Record<string, unknown>;
    expect(json).toEqual({
      challenge: "AQ",
      rpId: "example.com",
      allowCredentials: [{ type: "public-key", id: "Ag" }],
      userVerification: "required",
    });
  });
});

describe("credential", () => {
  it("rebuilds a registration with WebAuthn's methods", () => {
    const made = credential(
      {
        id: "AQ",
        rawId: "AQ",
        authenticatorAttachment: "platform",
        response: {
          clientDataJSON: "eyJ9",
          attestationObject: "o2Q",
          authenticatorData: "AAE",
          publicKey: "MFk",
          publicKeyAlgorithm: -7,
          transports: ["internal", "hybrid"],
        },
      },
      "create",
    ) as unknown as {
      id: string;
      rawId: ArrayBuffer;
      type: string;
      response: {
        clientDataJSON: ArrayBuffer;
        getTransports(): string[];
        getPublicKeyAlgorithm(): number;
        getAuthenticatorData(): ArrayBuffer;
      };
      getClientExtensionResults(): object;
    };
    expect(made.id).toBe("AQ");
    expect(new Uint8Array(made.rawId)).toEqual(bytes(1));
    expect(made.type).toBe("public-key");
    expect(new Uint8Array(made.response.clientDataJSON)).toEqual(
      new Uint8Array(buffer("eyJ9")),
    );
    expect(made.response.getTransports()).toEqual(["internal", "hybrid"]);
    expect(made.response.getPublicKeyAlgorithm()).toBe(-7);
    expect(new Uint8Array(made.response.getAuthenticatorData())).toEqual(
      bytes(0, 1),
    );
    expect(made.getClientExtensionResults()).toEqual({});
  });

  it("rebuilds an assertion, with the user handle", () => {
    const made = credential(
      {
        id: "AQ",
        rawId: "AQ",
        response: {
          clientDataJSON: "eyJ9",
          authenticatorData: "AAE",
          signature: "MEU",
          userHandle: "CQ",
        },
      },
      "get",
    ) as unknown as {
      response: { userHandle: ArrayBuffer; signature: ArrayBuffer };
    };
    expect(new Uint8Array(made.response.userHandle)).toEqual(bytes(9));
    expect(made.response.signature.byteLength).toBe(2);
  });
});

describe("passkey message guards", () => {
  it("accepts only well-formed calls of bounded size", () => {
    const call = {
      target: "background",
      from: "content",
      kind: "passkey",
      op: "get",
      request: "{}",
    };
    expect(isPasskeyCall(call)).toBe(true);
    expect(isPasskeyCall({ ...call, op: "delete" })).toBe(false);
    expect(isPasskeyCall({ ...call, request: "x".repeat(64 * 1024 + 1) })).toBe(
      false,
    );
    expect(isPasskeyCall({ ...call, from: "inline" })).toBe(false);
  });

  it("accepts the window's requests", () => {
    const base = { target: "background", from: "passkeyWindow", id: "1" };
    expect(isPasskeyWindowRequest({ ...base, kind: "prompt" })).toBe(true);
    expect(
      isPasskeyWindowRequest({ ...base, kind: "unlock", password: "p" }),
    ).toBe(true);
    expect(
      isPasskeyWindowRequest({
        ...base,
        kind: "get",
        vaultId: "v",
        itemId: "i",
      }),
    ).toBe(false);
    expect(isPasskeyWindowRequest({ ...base, kind: "steal" })).toBe(false);
  });
});

/**
 * WebAuthn options to their JSON form and answers back to credential
 * objects (D37), for the page-world wrapper. Pure, so it is tested in Node.
 */

type Binary = ArrayBuffer | ArrayBufferView;

function bytes(value: Binary): Uint8Array {
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
}

export function b64url(value: Binary): string {
  let binary = "";
  for (const byte of bytes(value)) binary += String.fromCharCode(byte);
  return btoa(binary)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

export function buffer(text: string): ArrayBuffer {
  const base64 = text.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(base64 + "=".repeat((4 - (base64.length % 4)) % 4));
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out.buffer;
}

function list(
  descriptors: readonly PublicKeyCredentialDescriptor[] | undefined,
): { type: string; id: string }[] | undefined {
  return descriptors?.map((d) => ({ type: d.type, id: b64url(d.id) }));
}

/** The JSON form of creation or request options (WebAuthn §5.1.8 / §5.1.9). */
export function toRequestJson(
  options:
    PublicKeyCredentialCreationOptions | PublicKeyCredentialRequestOptions,
): string {
  if ("user" in options) {
    return JSON.stringify({
      challenge: b64url(options.challenge),
      rp: { id: options.rp.id, name: options.rp.name },
      user: {
        id: b64url(options.user.id),
        name: options.user.name,
        displayName: options.user.displayName,
      },
      pubKeyCredParams: options.pubKeyCredParams.map((p) => ({
        type: p.type,
        alg: p.alg,
      })),
      timeout: options.timeout,
      excludeCredentials: list(options.excludeCredentials),
      authenticatorSelection: options.authenticatorSelection,
      attestation: options.attestation,
    });
  }
  return JSON.stringify({
    challenge: b64url(options.challenge),
    rpId: options.rpId,
    timeout: options.timeout,
    allowCredentials: list(options.allowCredentials),
    userVerification: options.userVerification,
  });
}

function own<T extends object>(target: T, props: Record<string, unknown>): T {
  for (const [key, value] of Object.entries(props)) {
    Object.defineProperty(target, key, { value, enumerable: true });
  }
  return target;
}

/**
 * A credential object from the core's response JSON. Own properties shadow
 * the native prototypes' getters, so `instanceof PublicKeyCredential` and
 * reading `response.clientDataJSON` both work.
 */
export function credential(
  json: Record<string, unknown>,
  op: "create" | "get",
): Credential {
  const r = json.response as Record<string, unknown>;
  const field = (name: string): ArrayBuffer => {
    const value = r[name];
    return buffer(typeof value === "string" ? value : "");
  };
  const response =
    op === "create"
      ? own(
          {},
          {
            clientDataJSON: field("clientDataJSON"),
            attestationObject: field("attestationObject"),
            getTransports: () => (r.transports as string[] | undefined) ?? [],
            getAuthenticatorData: () => field("authenticatorData"),
            getPublicKey: () => field("publicKey"),
            getPublicKeyAlgorithm: () => Number(r.publicKeyAlgorithm),
          },
        )
      : own(
          {},
          {
            clientDataJSON: field("clientDataJSON"),
            authenticatorData: field("authenticatorData"),
            signature: field("signature"),
            userHandle: r.userHandle === undefined ? null : field("userHandle"),
          },
        );
  const proto = globalThis as unknown as Record<
    string,
    { prototype: object } | undefined
  >;
  const responseProto =
    op === "create"
      ? proto.AuthenticatorAttestationResponse
      : proto.AuthenticatorAssertionResponse;
  if (responseProto) Object.setPrototypeOf(response, responseProto.prototype);
  const cred = own(
    {},
    {
      id: json.id,
      rawId: buffer(typeof json.rawId === "string" ? json.rawId : ""),
      type: "public-key",
      authenticatorAttachment: json.authenticatorAttachment ?? null,
      response,
      getClientExtensionResults: () => ({}),
      toJSON: () => json,
    },
  );
  if (proto.PublicKeyCredential)
    Object.setPrototypeOf(cred, proto.PublicKeyCredential.prototype);
  return cred as unknown as Credential;
}

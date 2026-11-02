/**
 * The page-world passkey wrapper (D37 §1), registered at document_start in
 * the page's own world when "Use PWM for passkeys" is on. It wraps
 * `navigator.credentials.create` and `.get` for `publicKey` requests only,
 * passes them to the isolated bridge by `postMessage`, and rebuilds the
 * answer as a credential. "Use another device or the browser", no passkey,
 * or anything it cannot handle goes to the browser's original calls,
 * unchanged. It runs beside the page's scripts, so nothing here is trusted
 * by the background.
 */
import { credential, toRequestJson } from "./convert.ts";

type Answer = { result?: string; fallback?: boolean; error?: string };

const creds = navigator.credentials as CredentialsContainer | undefined;
const marker = "__pwmPasskeys";

if (creds && !(marker in window)) {
  Object.defineProperty(window, marker, { value: true });
  const originalCreate = creds.create.bind(creds);
  const originalGet = creds.get.bind(creds);
  const pending = new Map<string, (answer: Answer) => void>();

  window.addEventListener("message", (event: MessageEvent) => {
    if (event.source !== window) return;
    const data = event.data as {
      source?: unknown;
      id?: unknown;
      answer?: Answer;
    };
    if (data.source !== "pwm-passkey-reply" || typeof data.id !== "string")
      return;
    const resolve = pending.get(data.id);
    if (!resolve) return;
    pending.delete(data.id);
    resolve(data.answer ?? { fallback: true });
  });

  const ask = (op: "create" | "get", request: string): Promise<Answer> =>
    new Promise((resolve) => {
      const id = crypto.randomUUID();
      pending.set(id, resolve);
      window.postMessage({ source: "pwm-passkey", id, op, request }, "*");
    });

  const settle = (answer: Answer, op: "create" | "get"): Credential | null => {
    if (answer.error !== undefined) {
      throw new DOMException(
        "The operation either timed out or was not allowed.",
        answer.error,
      );
    }
    if (answer.result === undefined) return null;
    return credential(JSON.parse(answer.result) as Record<string, unknown>, op);
  };

  creds.create = async function create(options?: CredentialCreationOptions) {
    const publicKey = options?.publicKey;
    if (!publicKey || options.signal?.aborted) return originalCreate(options);
    const answer = await ask("create", toRequestJson(publicKey));
    if (answer.fallback) return originalCreate(options);
    return settle(answer, "create");
  };

  creds.get = async function get(options?: CredentialRequestOptions) {
    const publicKey = options?.publicKey;
    // Conditional UI (passkeys in the username field) is the browser's, for now.
    if (
      !publicKey ||
      options.mediation === "conditional" ||
      options.signal?.aborted
    ) {
      return originalGet(options);
    }
    const answer = await ask("get", toRequestJson(publicKey));
    if (answer.fallback) return originalGet(options);
    return settle(answer, "get");
  };
}

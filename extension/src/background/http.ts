/**
 * Sends the core's requests (D28 steps) with `fetch` until the core is done
 * or asks to wait. Nothing here reads or logs bodies or headers: they can
 * carry tokens.
 */
import type { Core, CoreStep } from "./core.ts";

export type Fetch = (url: string, init: RequestInit) => Promise<Response>;

/** One answer, in the shape the core reads. */
export interface Answer {
  status: number;
  /** Names and values, alternating. */
  headers: string[];
  body: Uint8Array;
}

/** Sends the request of one `send` step; throws when there is no answer. */
export async function sendOnce(
  step: CoreStep,
  serverUrl: string,
  fetchFn: Fetch,
): Promise<Answer> {
  const headers = new Headers();
  for (let i = 0; i + 1 < step.headers.length; i += 2) {
    headers.append(step.headers[i] ?? "", step.headers[i + 1] ?? "");
  }
  const response = await fetchFn(
    serverUrl.replace(/\/+$/, "") + (step.path ?? "/"),
    {
      method: step.method ?? "GET",
      headers,
      body: step.body ? new Uint8Array(step.body) : null,
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
    },
  );
  const pairs: string[] = [];
  response.headers.forEach((value, name) => {
    pairs.push(name, value);
  });
  return {
    status: response.status,
    headers: pairs,
    body: new Uint8Array(await response.arrayBuffer()),
  };
}

/** Runs `first` and every step after it; returns the final wait/done step. */
export async function runSteps(
  core: Core,
  first: CoreStep,
  serverUrl: string,
  fetchFn: Fetch,
  now: () => number,
): Promise<CoreStep> {
  let step = first;
  const base = serverUrl.replace(/\/+$/, "");
  while (step.kind === "send") {
    const headers = new Headers();
    for (let i = 0; i + 1 < step.headers.length; i += 2) {
      headers.append(step.headers[i] ?? "", step.headers[i + 1] ?? "");
    }
    let response: Response;
    try {
      response = await fetchFn(base + (step.path ?? "/"), {
        method: step.method ?? "GET",
        headers,
        body: step.body ? new Uint8Array(step.body) : null,
        credentials: "omit",
        cache: "no-store",
        redirect: "error",
      });
    } catch {
      step = core.networkFailed();
      continue;
    }
    const pairs: string[] = [];
    response.headers.forEach((value, name) => {
      pairs.push(name, value);
    });
    const body = new Uint8Array(await response.arrayBuffer());
    step = core.receive(response.status, pairs, body, now());
  }
  return step;
}

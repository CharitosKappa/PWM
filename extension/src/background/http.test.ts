import { describe, expect, it } from "vitest";
import type { Core, CoreStep } from "./core.ts";
import { done, send } from "./fakes.ts";
import { runSteps } from "./http.ts";

/** A core that records what it receives and answers from a list. */
function recordingCore(answers: (() => CoreStep)[]) {
  const received: {
    status: number;
    headers: string[];
    body: string;
    now: number;
  }[] = [];
  let failures = 0;
  const next = (): CoreStep => {
    const answer = answers.shift();
    if (!answer) throw new Error("no more answers");
    return answer();
  };
  const core = {
    receive: (
      status: number,
      headers: string[],
      body: Uint8Array,
      now: number,
    ) => {
      received.push({
        status,
        headers,
        body: new TextDecoder().decode(body),
        now,
      });
      return next();
    },
    networkFailed: () => {
      failures += 1;
      return next();
    },
  } as unknown as Core;
  return { core, received, failures: () => failures };
}

describe("runSteps", () => {
  it("sends each request and hands the response back to the core", async () => {
    const { core, received } = recordingCore([
      () => send("/v1/two"),
      () => done("synced"),
    ]);
    const calls: { url: string; init: RequestInit }[] = [];
    const last = await runSteps(
      core,
      { ...send("/v1/one"), body: new TextEncoder().encode("{}") },
      "https://vault.example.org/",
      (url, init) => {
        calls.push({ url, init });
        return Promise.resolve(
          new Response("ok", { status: 201, headers: { "pwm-epoch": "3" } }),
        );
      },
      () => 42,
    );
    expect(last.outcome).toBe("synced");
    expect(calls.map((c) => c.url)).toEqual([
      "https://vault.example.org/v1/one",
      "https://vault.example.org/v1/two",
    ]);
    expect(calls[0]?.init).toMatchObject({
      method: "POST",
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
    });
    expect(new Headers(calls[0]?.init.headers).get("content-type")).toBe(
      "application/json",
    );
    expect(received[0]).toMatchObject({ status: 201, body: "ok", now: 42 });
    expect(received[0]?.headers).toEqual(
      expect.arrayContaining(["pwm-epoch", "3"]),
    );
  });

  it("tells the core when the network failed", async () => {
    const { core, failures } = recordingCore([() => done("synced")]);
    const last = await runSteps(
      core,
      send("/v1/one"),
      "https://vault.example.org",
      () => Promise.reject(new TypeError("offline")),
      () => 0,
    );
    expect(failures()).toBe(1);
    expect(last.kind).toBe("done");
  });

  it("returns at once when there is nothing to send", async () => {
    const { core } = recordingCore([]);
    const first = done("synced");
    expect(
      await runSteps(
        core,
        first,
        "https://x.example",
        () => Promise.reject(new Error("unused")),
        () => 0,
      ),
    ).toBe(first);
  });
});

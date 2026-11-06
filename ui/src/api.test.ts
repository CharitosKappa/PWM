import { beforeEach, describe as group, expect, test } from "vitest";
import { RequestError, describe, describeCode, send } from "./api.ts";
import { setHost } from "./host.ts";
import type { Host } from "./host.ts";
import type { Reply, Request } from "./protocol.ts";

/** A host that answers every request with `answer` and keeps what it was sent. */
function fakeHost(
  answer: Reply<unknown> | undefined,
): Host & { sent: Request[] } {
  const sent: Request[] = [];
  return {
    sent,
    send: (request) => {
      sent.push(request);
      return Promise.resolve(answer);
    },
    requestAccess: () => Promise.resolve(true),
    releaseAccess: () => Promise.resolve(),
    openManage: () => undefined,
    closeManage: null,
    browser: false,
    inlineUi: false,
    biometricName: null,
    manage: false,
    vaultWindow: false,
    asksServerAccess: false,
  };
}

group("send", () => {
  beforeEach(() => {
    setHost(fakeHost(undefined));
  });

  test("passes the request to the host and returns the reply's value", async () => {
    const host = fakeHost({ ok: true, value: { state: "signedOut" } });
    setHost(host);
    await expect(send({ kind: "status" })).resolves.toEqual({
      state: "signedOut",
    });
    expect(host.sent).toEqual([{ kind: "status" }]);
  });

  test("throws the host's error code", async () => {
    setHost(fakeHost({ ok: false, error: "wrong_password" }));
    const error = await send({ kind: "unlock", password: "x" }).catch(
      (e: unknown) => e,
    );
    expect(error).toBeInstanceOf(RequestError);
    expect((error as RequestError).code).toBe("wrong_password");
    expect(describe(error)).toBe("Wrong master password.");
  });

  test("a host that doesn't answer is an internal error", async () => {
    const error = await send({ kind: "lock" }).catch((e: unknown) => e);
    expect((error as RequestError).code).toBe("internal");
  });
});

test("an unknown code or a non-request error gets the plain fallback", () => {
  expect(describeCode("no_such_code")).toBe("Something went wrong. Try again.");
  expect(describe(new Error("boom"))).toBe("Something went wrong. Try again.");
});

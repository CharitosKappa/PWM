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

group("browser unlock texts", () => {
  test("name Touch ID and the Mac app, or Windows Hello and the Windows app (D69)", () => {
    setHost({
      ...fakeHost(undefined),
      browser: true,
      biometricName: "Touch ID",
    });
    expect(describeCode("helper_missing")).toContain("PWM app for Mac");
    expect(describeCode("touch_id_unavailable")).toContain("Touch ID");
    setHost({
      ...fakeHost(undefined),
      browser: true,
      biometricName: "Windows Hello",
    });
    for (const code of [
      "helper_missing",
      "touch_id_off",
      "touch_id_unavailable",
      "biometric_changed",
    ]) {
      expect(describeCode(code)).toContain("Windows Hello");
      expect(describeCode(code)).not.toContain("Touch ID");
    }
    expect(describeCode("helper_missing")).toContain("PWM app for Windows");
    // Other codes are the same everywhere.
    expect(describeCode("wrong_password")).toBe("Wrong master password.");
  });
});

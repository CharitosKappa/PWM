/**
 * Signing in again after the server refused this browser's session
 * (D50 §5): the master password only; the cache, the device and Touch ID
 * stay.
 */
import { describe, expect, it } from "vitest";
import {
  PASSWORD,
  done,
  type FakeCore,
  fakeFunctions,
  fakePlatform,
} from "./fakes.ts";
import { Background, ServiceError } from "./service.ts";

async function signedOutBySync() {
  const platform = fakePlatform();
  const fns = fakeFunctions();
  const background = await Background.start(fns, platform);
  await background.handle({
    kind: "signUp",
    email: "ada@example.com",
    password: PASSWORD,
  });
  const core = fns.cores.at(-1) as FakeCore;
  core.syncResult = () => done("signed_out");
  await background.handle({ kind: "sync" });
  return { background, platform, core };
}

describe("signing in again", () => {
  it("shows that the session ended", async () => {
    const { background } = await signedOutBySync();
    expect(await background.status()).toMatchObject({
      state: "unlocked",
      syncError: "signed_out",
    });
  });

  it("needs only the master password, and keeps the device and Touch ID", async () => {
    const { background, platform, core } = await signedOutBySync();
    await platform.disk.set("browserUnlock", "blob");
    const deviceId = await platform.disk.get("deviceId");
    core.syncResult = () => done("synced");

    const status = await background.handle({
      kind: "signInAgain",
      password: PASSWORD,
    });
    expect(status).toMatchObject({ state: "unlocked", syncError: null });
    expect(core.loginNames.at(-1)).toBe("");
    expect(await platform.disk.get("deviceId")).toBe(deviceId);
    expect(await platform.disk.get("browserUnlock")).toBe("blob");
    expect(await platform.disk.get("email")).toBe("ada@example.com");
  });

  it("says so when the password is wrong, and stays as it was", async () => {
    const { background } = await signedOutBySync();
    let code = "";
    try {
      await background.handle({ kind: "signInAgain", password: "nope" });
    } catch (error) {
      if (error instanceof ServiceError) code = error.code;
    }
    expect(code).toBe("invalid_credentials");
    expect(await background.status()).toMatchObject({
      syncError: "signed_out",
    });
  });

  it("needs a signed-in browser", async () => {
    const platform = fakePlatform();
    const background = await Background.start(fakeFunctions(), platform);
    let code = "";
    try {
      await background.handle({ kind: "signInAgain", password: PASSWORD });
    } catch (error) {
      if (error instanceof ServiceError) code = error.code;
    }
    expect(code).toBe("signed_out");
  });
});

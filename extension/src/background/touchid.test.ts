/**
 * Touch ID unlock through the PWM app for Mac (D48 §5), over the fake core
 * and a fake helper that answers as the real one does.
 */
import { describe, expect, it } from "vitest";
import { PASSWORD, fakeFunctions, fakePlatform } from "./fakes.ts";
import { Background, ServiceError } from "./service.ts";
import { MASTER_PASSWORD_EVERY_MS } from "./touchid.ts";

async function signedUp() {
  const platform = fakePlatform();
  const fns = fakeFunctions();
  const background = await Background.start(fns, platform);
  await background.handle({
    kind: "signUp",
    email: "ada@example.com",
    password: PASSWORD,
  });
  return { background, platform };
}

async function errorOf(promise: Promise<unknown>): Promise<string> {
  try {
    await promise;
  } catch (error) {
    if (error instanceof ServiceError) return error.code;
    throw error;
  }
  throw new Error("no error");
}

async function turnedOn() {
  const env = await signedUp();
  expect(await env.background.handle({ kind: "setTouchId", on: true })).toBe(
    true,
  );
  await env.background.handle({ kind: "lock" });
  return env;
}

describe("Touch ID unlock", () => {
  it("is off until turned on, then unlocks without the master password", async () => {
    const { background, platform } = await signedUp();
    expect(await background.handle({ kind: "touchId" })).toBe(false);
    expect(await background.handle({ kind: "setTouchId", on: true })).toBe(
      true,
    );
    // The key went to the helper once; only the blob is on disk.
    const enroll = platform.native.sent.at(-1);
    expect(enroll?.type).toBe("enroll");
    expect(enroll?.v).toBe(1);
    const stored = platform.disk.values.get("browserUnlock") ?? "";
    expect(stored).not.toContain(String(enroll?.key));
    expect(JSON.stringify([...platform.disk.values])).not.toContain(
      String(enroll?.key),
    );

    await background.handle({ kind: "lock" });
    expect(await background.status()).toMatchObject({
      state: "locked",
      touchId: true,
    });
    await background.handle({ kind: "unlockTouchId" });
    expect(await background.status()).toMatchObject({ state: "unlocked" });
    expect(platform.native.sent.at(-1)?.type).toBe("unlock");
  });

  it("asks for the master password every 14 days", async () => {
    const { background, platform } = await turnedOn();
    platform.clock.now += MASTER_PASSWORD_EVERY_MS;
    expect(await errorOf(background.handle({ kind: "unlockTouchId" }))).toBe(
      "master_password_due",
    );
    await background.handle({ kind: "unlock", password: PASSWORD });
    await background.handle({ kind: "lock" });
    await background.handle({ kind: "unlockTouchId" });
    expect(await background.status()).toMatchObject({ state: "unlocked" });
  });

  it("keeps it on after a cancel, and turns it off when Touch ID changed", async () => {
    const { background, platform } = await turnedOn();
    platform.native.touchId = "cancel";
    expect(await errorOf(background.handle({ kind: "unlockTouchId" }))).toBe(
      "biometric_cancelled",
    );
    expect(await background.handle({ kind: "touchId" })).toBe(true);

    platform.native.touchId = "changed";
    expect(await errorOf(background.handle({ kind: "unlockTouchId" }))).toBe(
      "biometric_changed",
    );
    expect(await background.handle({ kind: "touchId" })).toBe(false);
    expect(platform.disk.values.has("browserUnlock")).toBe(false);
    // The master password still unlocks.
    await background.handle({ kind: "unlock", password: PASSWORD });
    expect(await background.status()).toMatchObject({ state: "unlocked" });
  });

  it("says when the Mac app is missing, and turns on nothing", async () => {
    const { background, platform } = await signedUp();
    platform.native.installed = false;
    expect(
      await errorOf(background.handle({ kind: "setTouchId", on: true })),
    ).toBe("helper_missing");
    expect(await background.handle({ kind: "touchId" })).toBe(false);
  });

  it("is turned on only while unlocked", async () => {
    const { background } = await signedUp();
    await background.handle({ kind: "lock" });
    expect(
      await errorOf(background.handle({ kind: "setTouchId", on: true })),
    ).toBe("locked");
    expect(await errorOf(background.handle({ kind: "unlockTouchId" }))).toBe(
      "touch_id_off",
    );
  });

  it("forgets the key on the Mac when turned off or signing out", async () => {
    const { background, platform } = await signedUp();
    await background.handle({ kind: "setTouchId", on: true });
    expect(platform.native.keys.size).toBe(1);
    expect(await background.handle({ kind: "setTouchId", on: false })).toBe(
      false,
    );
    expect(platform.native.keys.size).toBe(0);

    await background.handle({ kind: "setTouchId", on: true });
    await background.handle({ kind: "signOut" });
    expect(platform.native.keys.size).toBe(0);
    expect(platform.disk.values.has("browserUnlock")).toBe(false);
    expect(platform.disk.values.has("passwordUnlockedAt")).toBe(false);
  });

  it("starts off after a new sign-in", async () => {
    const { background, platform } = await turnedOn();
    await background.handle({ kind: "unlock", password: PASSWORD });
    // A sign-in on this browser, as after signing out and in again.
    await background.handle({ kind: "signOut" });
    await background.handle({
      kind: "signUp",
      email: "bea@example.com",
      password: PASSWORD,
    });
    expect(await background.handle({ kind: "touchId" })).toBe(false);
    expect(platform.disk.values.has("browserUnlock")).toBe(false);
  });
});

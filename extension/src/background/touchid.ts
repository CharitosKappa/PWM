/**
 * Touch ID unlock through the PWM app for Mac (D48 §5). Turning it on makes
 * a blob in the core (the keyset under a fresh key, `pwm/browser-unlock`):
 * the blob stays on this browser's disk, and the key goes once to the
 * Mac's helper, which keeps it behind Touch ID. Unlocking asks the helper
 * for the key (Touch ID each time) and opens the blob; the KDF never runs.
 * The master password is still asked every 14 days and after each sign-in.
 */
import type { Core } from "./core.ts";
import { errorCode } from "./core.ts";
import { ServiceError } from "./errors.ts";
import type { Platform } from "./platform.ts";
import { fromBase64, toBase64 } from "../shared/bytes.ts";

/** The helper's protocol version (`BrowserUnlock.version` in PWMKit). */
const VERSION = 1;
/** The master password is asked again after this long (as D45). */
export const MASTER_PASSWORD_EVERY_MS = 14 * 24 * 60 * 60 * 1000;

interface Stored {
  blob: string;
  accountId: string;
}

interface HelperReply {
  ok: boolean;
  key: string | undefined;
  error: string | undefined;
}

interface TouchIdDeps {
  core: () => Core;
  platform: Platform;
  deviceId: () => Promise<string>;
  sealedTokens: () => Promise<Uint8Array | undefined>;
}

export class TouchIdUnlock {
  constructor(private readonly deps: TouchIdDeps) {}

  /** Turned on for the signed-in account. */
  async isOn(): Promise<boolean> {
    return (await this.stored()) !== undefined;
  }

  /** While unlocked: makes the blob and enrols its key with the helper. */
  async turnOn(): Promise<void> {
    const core = this.deps.core();
    const accountId = core.accountId();
    if (accountId === undefined || !core.isUnlocked())
      throw new ServiceError("locked");
    const deviceId = await this.deps.deviceId();
    let bytes: Uint8Array;
    try {
      bytes = core.exportBrowserUnlock(deviceId);
    } catch (error) {
      throw new ServiceError(errorCode(error));
    }
    const key = bytes.slice(0, 32);
    const blob = bytes.slice(32);
    const reply = await this.ask({
      type: "enroll",
      account_id: accountId,
      device_id: deviceId,
      key: toBase64(key),
    });
    key.fill(0);
    if (!reply.ok) throw new ServiceError(helperError(reply.error));
    const stored: Stored = { blob: toBase64(blob), accountId };
    await this.deps.platform.disk.set("browserUnlock", JSON.stringify(stored));
  }

  /** Forgets the blob here and, as far as it can, the key on the Mac. */
  async turnOff(): Promise<void> {
    const stored = await this.stored();
    await this.deps.platform.disk.remove(["browserUnlock"]);
    if (stored === undefined) return;
    try {
      await this.ask({
        type: "forget",
        account_id: stored.accountId,
        device_id: await this.deps.deviceId(),
      });
    } catch {
      // The app may be gone; the blob is useless without its key anyway.
    }
  }

  /** The master password was typed: the 14 days start again. */
  async passwordTyped(): Promise<void> {
    await this.deps.platform.disk.set(
      "passwordUnlockedAt",
      String(this.deps.platform.now()),
    );
  }

  /** Touch ID on the Mac, then the blob opens. */
  async unlock(): Promise<void> {
    const stored = await this.stored();
    if (stored === undefined) throw new ServiceError("touch_id_off");
    const last = Number(
      (await this.deps.platform.disk.get("passwordUnlockedAt")) ?? "0",
    );
    if (this.deps.platform.now() - last >= MASTER_PASSWORD_EVERY_MS)
      throw new ServiceError("master_password_due");
    const deviceId = await this.deps.deviceId();
    const reply = await this.ask({
      type: "unlock",
      account_id: stored.accountId,
      device_id: deviceId,
    });
    if (!reply.ok || reply.key === undefined) {
      const code = helperError(reply.error);
      // The Mac forgot the key (a new face or finger): off here too.
      if (code === "biometric_changed") await this.turnOff();
      throw new ServiceError(code);
    }
    const key = fromBase64(reply.key);
    try {
      this.deps
        .core()
        .unlockBrowser(
          fromBase64(stored.blob),
          key,
          deviceId,
          await this.deps.sealedTokens(),
        );
    } catch (error) {
      const code = errorCode(error);
      if (code === "browser_unlock_failed") {
        await this.turnOff();
        throw new ServiceError("biometric_changed");
      }
      throw new ServiceError(code);
    } finally {
      key.fill(0);
    }
  }

  private async stored(): Promise<Stored | undefined> {
    const text = await this.deps.platform.disk.get("browserUnlock");
    if (text === undefined) return undefined;
    try {
      const value = JSON.parse(text) as Partial<Stored>;
      if (typeof value.blob !== "string" || typeof value.accountId !== "string")
        return undefined;
      if (value.accountId !== this.deps.core().accountId()) return undefined;
      return { blob: value.blob, accountId: value.accountId };
    } catch {
      return undefined;
    }
  }

  private async ask(message: Record<string, string>): Promise<HelperReply> {
    let reply: unknown;
    try {
      reply = await this.deps.platform.native.send({ v: VERSION, ...message });
    } catch {
      throw new ServiceError("helper_missing");
    }
    if (typeof reply !== "object" || reply === null)
      throw new ServiceError("helper_missing");
    const { ok, key, error } = reply as Record<string, unknown>;
    return {
      ok: ok === true,
      key: typeof key === "string" ? key : undefined,
      error: typeof error === "string" ? error : undefined,
    };
  }
}

/** The helper's error, as the popup's codes. */
function helperError(error: string | undefined): string {
  switch (error) {
    case "cancelled":
      return "biometric_cancelled";
    case "changed":
    case "unknown":
      return "biometric_changed";
    case "unavailable":
      return "touch_id_unavailable";
    default:
      return "helper_missing";
  }
}

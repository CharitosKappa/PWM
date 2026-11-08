/** Server address (hosted or self-hosted) and the auto-lock timer (D34). */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import { HOSTED_SERVER } from "./config.ts";
import { AUTO_LOCK_CHOICES } from "./protocol.ts";
import type { Settings as SettingsValue, Status } from "./protocol.ts";
import { describe, send } from "./api.ts";
import { host } from "./host.ts";
import { InlineIconSetting, PasskeysSetting } from "./InlineIcon.tsx";
import { PlanSetting } from "./Plan.tsx";
import { Field, Notice } from "./ui.tsx";

const LOCK_WORDS: Record<number, string> = {
  1: "1 minute",
  5: "5 minutes",
  15: "15 minutes",
  60: "1 hour",
  480: "8 hours",
};

export function Settings({
  status,
  onBack,
}: {
  status: Status;
  onBack: () => void;
}): JSX.Element {
  const [settings, setSettings] = useState<SettingsValue | null>(null);
  const [server, setServer] = useState("");
  const [selfHosted, setSelfHosted] = useState(false);
  const [message, setMessage] = useState<{
    kind: "danger" | "info";
    text: string;
  } | null>(null);

  useEffect(() => {
    void send({ kind: "settings" }).then((value) => {
      setSettings(value);
      setSelfHosted(value.serverUrl !== HOSTED_SERVER);
      setServer(value.serverUrl === HOSTED_SERVER ? "" : value.serverUrl);
    });
  }, []);

  if (settings === null) return <main class="stack" />;

  const save = async (next: SettingsValue): Promise<void> => {
    setMessage(null);
    try {
      if (next.serverUrl !== HOSTED_SERVER) {
        // Ask for this one host only, now (D34); the manifest has no broad host access.
        const origin = `${new URL(next.serverUrl).origin}/*`;
        if (!(await host().requestAccess({ origins: [origin] }))) {
          setMessage({
            kind: "danger",
            text: "The extension needs permission to reach your server.",
          });
          return;
        }
      }
      setSettings(await send({ kind: "setSettings", settings: next }));
      setMessage({ kind: "info", text: "Saved." });
    } catch (err) {
      setMessage({ kind: "danger", text: describe(err) });
    }
  };

  const signedOut = status.state === "signedOut";

  return (
    <main class="stack">
      <button class="link" onClick={onBack}>
        ← Back
      </button>
      <h2>Settings</h2>

      <label class="field">
        <span class="label">Lock after</span>
        <select
          value={String(settings.autoLockMinutes)}
          onChange={(event) => {
            void save({
              ...settings,
              autoLockMinutes: Number(event.currentTarget.value),
            });
          }}
        >
          {AUTO_LOCK_CHOICES.map((minutes) => (
            <option key={minutes} value={String(minutes)}>
              {LOCK_WORDS[minutes]} of inactivity
            </option>
          ))}
        </select>
        <span class="caption muted">
          {host().browser
            ? "It also locks when your computer locks or the browser closes."
            : "It also locks when your computer locks or sleeps."}
        </span>
      </label>

      {status.state === "unlocked" && <PlanSetting />}

      {host().browser && <InlineIconSetting />}
      {host().browser && <PasskeysSetting />}
      {status.state === "unlocked" && host().biometricName !== null && (
        <BiometricSetting name={host().biometricName ?? ""} />
      )}
      {status.state === "unlocked" &&
        !host().browser &&
        host().biometricName !== null && (
          <BrowserUnlockSetting name={host().biometricName ?? ""} />
        )}

      <fieldset class="stack" disabled={!signedOut}>
        <legend class="label">Server</legend>
        <label class="row">
          <input
            type="radio"
            checked={!selfHosted}
            onChange={() => {
              setSelfHosted(false);
            }}
          />
          Hosted
        </label>
        <label class="row">
          <input
            type="radio"
            checked={selfHosted}
            onChange={() => {
              setSelfHosted(true);
            }}
          />
          Self-hosted
        </label>
        {selfHosted && (
          <Field
            label="Server address"
            type="url"
            value={server}
            onInput={setServer}
          />
        )}
        <button
          onClick={() =>
            void save({
              ...settings,
              serverUrl: selfHosted ? server.trim() : HOSTED_SERVER,
            })
          }
          disabled={selfHosted && server.trim() === ""}
        >
          Save server
        </button>
        {!signedOut && (
          <span class="caption muted">Sign out to change the server.</span>
        )}
      </fieldset>

      <div class="slot">
        {message && <Notice kind={message.kind}>{message.text}</Notice>}
      </div>

      {!signedOut && host().manage && (
        <button
          id="open-manage"
          onClick={() => {
            host().openManage();
          }}
        >
          Password health, devices and export
        </button>
      )}

      {!signedOut && (
        <button
          class="danger"
          onClick={() =>
            void send({ kind: "signOut" }).then(() => {
              onBack();
            })
          }
        >
          Sign out of this device
        </button>
      )}

      {!signedOut && host().manage && (
        <button
          class="link"
          id="open-delete-account"
          onClick={() => {
            host().openManage("account");
          }}
        >
          Delete account
        </button>
      )}
    </main>
  );
}

/**
 * "Unlock with Touch ID" in the extension: asks for the native-messaging
 * permission on the click (D44), then enrols with the Mac app's helper,
 * which asks for Touch ID once. "Unlock with Windows Hello" on the desktop
 * (D68): Windows asks for a face, a finger or the PIN. "Unlock with Linux
 * login" (D71): polkit asks for the login password or a fingerprint.
 */
/**
 * The desktop's "Allow browser extensions to unlock with Windows Hello"
 * (D69 §4) or "with Linux login" (D71 §4): registers PWM's browser helper
 * for this user. Off removes it and everything it kept.
 */
function BrowserUnlockSetting({ name }: { name: string }): JSX.Element {
  const [on, setOn] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void send({ kind: "browserUnlock" }).then(setOn);
  }, []);

  const change = async (next: boolean): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      setOn(await send({ kind: "setBrowserUnlock", on: next }));
    } catch (err) {
      setError(describe(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="stack">
      <label class="row">
        <input
          type="checkbox"
          id="browser-unlock"
          checked={on === true}
          disabled={busy || on === null}
          onChange={(event) => void change(event.currentTarget.checked)}
        />
        Allow browser extensions to unlock with {name}
      </label>
      <span class="caption muted">
        {name === "Linux login"
          ? "For PWM in Chrome, Chromium, Edge, Brave, Vivaldi and Firefox, installed as packages (not Snap or Flatpak)."
          : "For PWM in Chrome, Edge, Brave, Opera and Firefox."}{" "}
        Then turn on "Unlock with {name}" in the extension's Settings.
      </span>
      {error && <Notice kind="danger">{error}</Notice>}
    </div>
  );
}

function BiometricSetting({ name }: { name: string }): JSX.Element {
  const [on, setOn] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The extension unlocks through the PWM app for its computer (D48, D69, D71).
  const app =
    name === "Windows Hello"
      ? "Windows"
      : name === "Linux login"
        ? "Linux"
        : "Mac";

  useEffect(() => {
    void send({ kind: "touchId" }).then(setOn);
  }, []);

  const change = async (next: boolean): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      if (
        next &&
        host().browser &&
        !(await host().requestAccess({
          permissions: ["nativeMessaging"],
        }))
      ) {
        setError(`PWM needs permission to talk to the PWM app for ${app}.`);
        return;
      }
      setOn(await send({ kind: "setTouchId", on: next }));
    } catch (err) {
      setError(describe(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="stack">
      <label class="row">
        <input
          type="checkbox"
          id="touch-id"
          checked={on === true}
          disabled={busy || on === null}
          onChange={(event) => void change(event.currentTarget.checked)}
        />
        Unlock with {name}
      </label>
      <span class="caption muted">
        {host().browser
          ? `Needs the PWM app for ${app}, with browser unlock turned on in its Settings.`
          : name === "Linux login"
            ? "Your computer asks for your login password, or your fingerprint where it's set up."
            : "Windows asks for your face, fingerprint or PIN."}{" "}
        Your master password is still asked every 14 days.
      </span>
      {error && <Notice kind="danger">{error}</Notice>}
    </div>
  );
}

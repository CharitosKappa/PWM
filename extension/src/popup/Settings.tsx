/** Server address (hosted or self-hosted) and the auto-lock timer (D34). */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import { HOSTED_SERVER } from "../shared/config.ts";
import { AUTO_LOCK_CHOICES } from "../shared/messages.ts";
import type { Settings as SettingsValue, Status } from "../shared/messages.ts";
import { describe, send } from "./api.ts";
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
        if (!(await chrome.permissions.request({ origins: [origin] }))) {
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
          It also locks when your computer locks or the browser closes.
        </span>
      </label>

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
    </main>
  );
}

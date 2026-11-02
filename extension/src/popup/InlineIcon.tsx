/**
 * The "Fill and save on sign-in pages" switch (D35 §1, D36 §1): the inline
 * icon and save offers. Turning it on asks for site access at
 * that moment (it must be the first thing the click does); turning it off
 * gives the access back.
 */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import { HOSTED_SERVER, SITE_ACCESS } from "../shared/config.ts";
import { send } from "./api.ts";

export async function enableInlineIcon(): Promise<boolean> {
  if (!(await chrome.permissions.request({ origins: SITE_ACCESS }))) {
    return false;
  }
  return send({ kind: "setInlineIcon", on: true });
}

/** Gives site access back, unless a self-hosted server needs it. */
export async function disableInlineIcon(): Promise<void> {
  await send({ kind: "setInlineIcon", on: false });
  const settings = await send({ kind: "settings" });
  if (settings.serverUrl === HOSTED_SERVER) {
    await chrome.permissions.remove({ origins: SITE_ACCESS });
  }
}

/** The checkbox in Settings. */
export function InlineIconSetting(): JSX.Element {
  const [on, setOn] = useState<boolean | null>(null);

  useEffect(() => {
    send({ kind: "inlineIcon" })
      .then(setOn)
      .catch(() => {
        setOn(false);
      });
  }, []);

  const toggle = (): void => {
    if (on === null) return;
    if (on) {
      void disableInlineIcon().then(() => {
        setOn(false);
      });
    } else {
      void enableInlineIcon().then(setOn);
    }
  };

  return (
    <label class="field">
      <span class="row">
        <input
          type="checkbox"
          checked={on === true}
          disabled={on === null}
          onChange={toggle}
        />
        <span class="label">Fill and save on sign-in pages</span>
      </span>
      <span class="caption muted">
        Shows the icon in sign-in fields, and offers to save a login after you
        sign in. The browser will ask to let PWM read sites. It reads what you
        type only when you sign in.
      </span>
    </label>
  );
}

/** Offered once, after sign-up (D35 §1). */
export function InlineIconOffer({
  onDone,
}: {
  onDone: () => void;
}): JSX.Element {
  return (
    <div class="stack">
      <h2>Fill and save on sign-in pages?</h2>
      <p>
        Fill with one click on the icon, and save new logins when you sign in.
        The browser will ask to let PWM read sites. You can change this in
        Settings.
      </p>
      <div class="row">
        <button
          class="primary"
          onClick={() => {
            void enableInlineIcon().finally(onDone);
          }}
        >
          Turn on
        </button>
        <button onClick={onDone}>Not now</button>
      </div>
    </div>
  );
}

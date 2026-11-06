/**
 * The "Fill and save on sign-in pages" switch (D35 §1, D36 §1): the inline
 * icon and save offers. Turning it on asks for site access at
 * that moment (it must be the first thing the click does); turning it off
 * gives the access back.
 */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import { HOSTED_SERVER, SITE_ACCESS } from "./config.ts";
import { describe, send } from "./api.ts";
import { host } from "./host.ts";
import { Notice } from "./ui.tsx";

export async function enableInlineIcon(): Promise<boolean> {
  if (!(await host().requestAccess({ origins: SITE_ACCESS }))) {
    return false;
  }
  return send({ kind: "setInlineIcon", on: true });
}

/**
 * Gives site access back once nothing uses it: neither switch is on and
 * the server is the hosted one (a self-hosted server may need the access).
 */
async function releaseSiteAccess(): Promise<void> {
  const [icon, passkeys, settings] = await Promise.all([
    send({ kind: "inlineIcon" }),
    send({ kind: "passkeys" }),
    send({ kind: "settings" }),
  ]);
  if (!icon && !passkeys && settings.serverUrl === HOSTED_SERVER) {
    await host().releaseAccess({ origins: SITE_ACCESS });
  }
}

export async function disableInlineIcon(): Promise<void> {
  await send({ kind: "setInlineIcon", on: false });
  await releaseSiteAccess();
}

/** "Use PWM for passkeys" (D37 §1): the same site access, asked for at that moment. */
export function PasskeysSetting(): JSX.Element {
  const [on, setOn] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    send({ kind: "passkeys" })
      .then(setOn)
      .catch(() => {
        setOn(false);
      });
  }, []);

  const toggle = (): void => {
    if (on === null) return;
    if (on) {
      void send({ kind: "setPasskeys", on: false })
        .then(releaseSiteAccess)
        .then(() => {
          setOn(false);
        });
      return;
    }
    // The permission request must be the first thing the click does.
    setError(null);
    void host()
      .requestAccess({ origins: SITE_ACCESS })
      .then((granted) =>
        granted ? send({ kind: "setPasskeys", on: true }) : false,
      )
      .then(setOn)
      .catch(async (err: unknown) => {
        // A browser that cannot run the page script (D40 §2).
        setError(describe(err));
        setOn(false);
        await releaseSiteAccess();
      });
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
        <span class="label">Use PWM for passkeys</span>
      </span>
      <span class="caption muted">
        Sites that ask for a passkey get one from your vault, after you confirm
        in a PWM window. You can always use another device or the browser
        instead.
      </span>
      {error && <Notice kind="warning">{error}</Notice>}
    </label>
  );
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
        <span class="label">
          {host().inlineUi
            ? "Fill and save on sign-in pages"
            : "Save logins when you sign in"}
        </span>
      </span>
      <span class="caption muted">
        {host().inlineUi
          ? "Shows the icon in sign-in fields, and offers to save a login after you sign in."
          : "Offers to save a login after you sign in, on the toolbar button."}{" "}
        The browser will ask to let PWM read sites. It reads what you type only
        when you sign in.
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
      <h2>
        {host().inlineUi
          ? "Fill and save on sign-in pages?"
          : "Save logins when you sign in?"}
      </h2>
      <p>
        {host().inlineUi
          ? "Fill with one click on the icon, and save new logins when you sign in."
          : "PWM offers to save new logins when you sign in."}{" "}
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

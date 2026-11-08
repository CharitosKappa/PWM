/** The popup: picks the screen from the background's status (D34). */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import type { Status } from "./protocol.ts";
import { describe, send } from "./api.ts";
import { host } from "./host.ts";
import { Report, ReportLink } from "./Report.tsx";
import { Settings } from "./Settings.tsx";
import { SignedOut } from "./SignedOut.tsx";
import { Field, Notice } from "./ui.tsx";
import { Vault } from "./Vault.tsx";
import { VaultWindow } from "./vault-window/VaultWindow.tsx";

export function App({
  startInSettings,
}: {
  startInSettings: boolean;
}): JSX.Element {
  const [status, setStatus] = useState<Status | null>(null);
  const [settings, setSettings] = useState(startInSettings);
  const [report, setReport] = useState(false);
  const openReport = (): void => {
    setReport(true);
  };

  const refresh = (): void => {
    send({ kind: "status" })
      .then(setStatus)
      .catch(() => {
        setStatus({ state: "signedOut" });
      });
  };

  useEffect(refresh, []);

  if (status === null) return <main class="stack" aria-busy="true" />;
  if (report) {
    return (
      <Report
        onDone={() => {
          setReport(false);
        }}
      />
    );
  }
  if (settings) {
    return (
      <Settings
        status={status}
        onBack={() => {
          setSettings(false);
          refresh();
        }}
      />
    );
  }
  switch (status.state) {
    case "signedOut":
      return (
        <>
          <SignedOut onDone={refresh} ended={status.ended} />
          <footer class="row">
            <button
              class="link"
              onClick={() => {
                setSettings(true);
              }}
            >
              Settings
            </button>
          </footer>
        </>
      );
    case "locked":
      return (
        <Unlock
          email={status.email}
          touchId={status.touchId}
          onDone={refresh}
          onReport={openReport}
        />
      );
    case "unlocked":
      if (host().vaultWindow) {
        return (
          <VaultWindow
            status={status}
            onStatus={setStatus}
            onSettings={() => {
              setSettings(true);
            }}
          />
        );
      }
      return (
        <Vault
          status={status}
          onStatus={setStatus}
          onSettings={() => {
            setSettings(true);
          }}
          onReport={openReport}
        />
      );
  }
}

/** Also the manage page's, when the vault is locked (D44 §1). */
export function Unlock({
  email,
  touchId = false,
  onDone,
  onReport,
}: {
  email: string;
  /** The platform's biometric unlock is on (Touch ID, Windows Hello). */
  touchId?: boolean;
  onDone: () => void;
  onReport?: () => void;
}): JSX.Element {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [offer, setOffer] = useState(false);
  const name = host().biometricName ?? "Touch ID";

  const submit = async (event: Event): Promise<void> => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await send({ kind: "unlock", password });
      setPassword("");
      // The one offer after a master password unlock (D45 §3).
      if (await send({ kind: "biometricOffer" }).catch(() => false)) {
        setOffer(true);
        return;
      }
      onDone();
    } catch (err) {
      setError(describe(err));
    } finally {
      setBusy(false);
    }
  };

  const withTouchId = async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await send({ kind: "unlockTouchId" });
      onDone();
    } catch (err) {
      setError(describe(err));
    } finally {
      setBusy(false);
    }
  };

  // On the desktop the unlock screen tries it once by itself (D45, D68).
  useEffect(() => {
    if (touchId && !host().browser) void withTouchId();
  }, []);

  if (offer) {
    const answer = async (on: boolean): Promise<void> => {
      setBusy(true);
      setError(null);
      try {
        if (on) await send({ kind: "setTouchId", on: true });
        else await send({ kind: "dismissBiometricOffer" });
        onDone();
      } catch (err) {
        setError(describe(err));
      } finally {
        setBusy(false);
      }
    };
    return (
      <main class="stack">
        <h1>Use {name} next time?</h1>
        <p>
          Unlock with {name} instead of typing your master password. It's still
          asked every 14 days.
        </p>
        <div class="slot">
          {error && <Notice kind="danger">{error}</Notice>}
        </div>
        <div class="row">
          <button
            class="primary"
            disabled={busy}
            onClick={() => void answer(true)}
          >
            Turn on
          </button>
          <button disabled={busy} onClick={() => void answer(false)}>
            Not now
          </button>
        </div>
      </main>
    );
  }

  return (
    <main class="stack">
      <h1>Unlock</h1>
      <p class="muted">{email}</p>
      {touchId && (
        <button
          class="primary"
          id="unlock-touch-id"
          disabled={busy}
          onClick={() => void withTouchId()}
        >
          Unlock with {name}
        </button>
      )}
      <form class="stack" onSubmit={(e) => void submit(e)}>
        <Field
          label="Master password"
          type="password"
          value={password}
          onInput={setPassword}
          autoComplete="current-password"
        />
        <div class="slot">
          {error && <Notice kind="danger">{error}</Notice>}
        </div>
        <button class="primary" disabled={busy || password === ""}>
          {busy ? "Unlocking…" : "Unlock"}
        </button>
      </form>
      {onReport && host().browser && <ReportLink onOpen={onReport} />}
    </main>
  );
}

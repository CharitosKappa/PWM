/** The popup: picks the screen from the background's status (D34). */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import type { Status } from "../shared/messages.ts";
import { describe, send } from "./api.ts";
import { Report, ReportLink } from "./Report.tsx";
import { Settings } from "./Settings.tsx";
import { SignedOut } from "./SignedOut.tsx";
import { Field, Notice } from "./ui.tsx";
import { Vault } from "./Vault.tsx";

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
          <SignedOut onDone={refresh} />
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
        <Unlock email={status.email} onDone={refresh} onReport={openReport} />
      );
    case "unlocked":
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

function Unlock({
  email,
  onDone,
  onReport,
}: {
  email: string;
  onDone: () => void;
  onReport: () => void;
}): JSX.Element {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: Event): Promise<void> => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await send({ kind: "unlock", password });
      setPassword("");
      onDone();
    } catch (err) {
      setError(describe(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main class="stack">
      <h1>Unlock</h1>
      <p class="muted">{email}</p>
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
      <ReportLink onOpen={onReport} />
    </main>
  );
}

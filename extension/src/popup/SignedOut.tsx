/** Sign in, create an account, and the one-time Secret Key screen (D34). */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import {
  ASKS_SERVER_ACCESS,
  askServerAccess,
  useServerOrigin,
} from "./access.ts";
import { fromBase64 } from "../shared/bytes.ts";
import type { AskView } from "../shared/messages.ts";
import { describe, describeCode, send } from "./api.ts";
import { InlineIconOffer } from "./InlineIcon.tsx";
import { Field, Notice } from "./ui.tsx";

const MIN_SCORE = 3;
const SCORE_WORDS = ["Very weak", "Weak", "Fair", "Strong", "Very strong"];

export function SignedOut({ onDone }: { onDone: () => void }): JSX.Element {
  const [mode, setMode] = useState<"signIn" | "signUp">("signIn");
  return (
    <main class="stack">
      <h1>Password manager</h1>
      <div class="tabs" role="tablist">
        <button
          role="tab"
          aria-selected={mode === "signIn"}
          onClick={() => {
            setMode("signIn");
          }}
        >
          Sign in
        </button>
        <button
          role="tab"
          aria-selected={mode === "signUp"}
          onClick={() => {
            setMode("signUp");
          }}
        >
          Create account
        </button>
      </div>
      {mode === "signIn" ? (
        <SignIn onDone={onDone} />
      ) : (
        <SignUp onDone={onDone} />
      )}
    </main>
  );
}

function SignIn({ onDone }: { onDone: () => void }): JSX.Element {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [secretKey, setSecretKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Asking a signed-in device (D46): its state, and the code to compare. */
  const [ask, setAsk] = useState<AskView | null>(null);
  const origin = useServerOrigin();
  const approved = ask?.state === "approved";

  // While asking, the popup polls every 2 seconds.
  useEffect(() => {
    if (ask === null || ask.state === "approved" || ask.state === "ended")
      return;
    const timer = setTimeout(() => {
      send({ kind: "askApprovalPoll" })
        .then(setAsk)
        .catch((err: unknown) => {
          setError(describe(err));
          setAsk(null);
        });
    }, 2000);
    return () => {
      clearTimeout(timer);
    };
  }, [ask]);

  const startAsking = async (): Promise<void> => {
    if (!(await askServerAccess(origin))) {
      setError(describeCode("no_server_access"));
      return;
    }
    setError(null);
    try {
      setAsk(await send({ kind: "askApproval", email: email.trim() }));
    } catch (err) {
      setError(describe(err));
    }
  };

  const stopAsking = (): void => {
    void send({ kind: "askApprovalCancel" });
    setAsk(null);
  };

  const submit = async (event: Event): Promise<void> => {
    event.preventDefault();
    if (!(await askServerAccess(origin))) {
      setError(describeCode("no_server_access"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (approved) {
        await send({ kind: "signInApproved", password });
      } else {
        await send({
          kind: "signIn",
          email: email.trim(),
          password,
          secretKey: secretKey.trim(),
        });
      }
      setPassword("");
      onDone();
    } catch (err) {
      setError(describe(err));
    } finally {
      setBusy(false);
    }
  };

  if (ask !== null && !approved) {
    return (
      <div class="stack" id="asking">
        <p>
          Approve this request on a device where you're signed in: Approve a new
          device in the app's Settings, or Devices on the extension's manage
          page.
        </p>
        {ask.state === "code" && ask.code !== null ? (
          <>
            <p class="caption muted">Check that it shows this code:</p>
            <p class="secret" id="approval-code">
              {ask.code}
            </p>
          </>
        ) : ask.state === "ended" ? (
          <Notice kind="info">
            The request ended. Ask again, or sign in with your Secret Key.
          </Notice>
        ) : (
          <p class="caption muted">Waiting for your other device…</p>
        )}
        <button onClick={stopAsking}>
          {ask.state === "ended" ? "Back" : "Cancel"}
        </button>
      </div>
    );
  }

  return (
    <form class="stack" onSubmit={(e) => void submit(e)}>
      <Field
        label="Email"
        type="email"
        value={email}
        onInput={setEmail}
        autoComplete="username"
      />
      <Field
        label="Master password"
        type="password"
        value={password}
        onInput={setPassword}
        autoComplete="current-password"
      />
      {approved ? (
        <Notice kind="info">Secret Key received from your other device.</Notice>
      ) : (
        <>
          <Field
            label="Secret Key"
            value={secretKey}
            onInput={setSecretKey}
            mono
            hint="On your Emergency Kit."
          />
          <button
            type="button"
            id="ask-device"
            disabled={!email.includes("@")}
            onClick={() => void startAsking()}
          >
            Sign in with another device instead
          </button>
        </>
      )}
      <ServerAccessNote />
      <div class="slot">{error && <Notice kind="danger">{error}</Notice>}</div>
      <button
        class="primary"
        disabled={busy || !email || !password || (!secretKey && !approved)}
      >
        {busy ? "Signing in…" : "Sign in"}
      </button>
    </form>
  );
}

function SignUp({ onDone }: { onDone: () => void }): JSX.Element {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [score, setScore] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [secretKey, setSecretKey] = useState<string | null>(null);
  const [offer, setOffer] = useState(false);
  const origin = useServerOrigin();

  useEffect(() => {
    if (password === "") {
      setScore(null);
      return;
    }
    let current = true;
    const timer = setTimeout(() => {
      send({ kind: "score", password, email })
        .then((value) => {
          if (current) setScore(value);
        })
        .catch(() => {
          if (current) setScore(null);
        });
    }, 150);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [password, email]);

  if (offer) return <InlineIconOffer onDone={onDone} />;
  if (secretKey !== null)
    return (
      <SecretKey
        secretKey={secretKey}
        onDone={() => {
          setOffer(true);
        }}
      />
    );

  const strongEnough = score !== null && score >= MIN_SCORE;
  const matches = password === confirm;

  const submit = async (event: Event): Promise<void> => {
    event.preventDefault();
    if (!(await askServerAccess(origin))) {
      setError(describeCode("no_server_access"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const reply = await send({
        kind: "signUp",
        email: email.trim(),
        password,
      });
      setPassword("");
      setConfirm("");
      setSecretKey(reply.secretKey);
    } catch (err) {
      setError(describe(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form class="stack" onSubmit={(e) => void submit(e)}>
      <Field
        label="Email"
        type="email"
        value={email}
        onInput={setEmail}
        autoComplete="username"
      />
      <Field
        label="Master password"
        type="password"
        value={password}
        onInput={setPassword}
        autoComplete="new-password"
      />
      <div class="meter" data-score={score ?? ""} aria-live="polite">
        <div class="meter-track">
          <div
            class="meter-bar"
            style={{
              width: score === null ? "0%" : `${String((score + 1) * 20)}%`,
            }}
          />
        </div>
        <span class="caption">
          {score === null ? " " : (SCORE_WORDS[score] ?? "")}
        </span>
      </div>
      <Field
        label="Confirm master password"
        type="password"
        value={confirm}
        onInput={setConfirm}
        autoComplete="new-password"
      />
      <div class="slot">
        {confirm !== "" && !matches && (
          <Notice kind="warning">The passwords don't match.</Notice>
        )}
      </div>
      <p class="caption muted">
        Nobody can reset your master password, not even us. Choose one you will
        remember.
      </p>
      <ServerAccessNote />
      <div class="slot">{error && <Notice kind="danger">{error}</Notice>}</div>
      <button
        class="primary"
        disabled={busy || !email || !strongEnough || !matches}
      >
        {busy ? "Creating account…" : "Create account"}
      </button>
    </form>
  );
}

/** Says why Firefox asks for access when the form is sent (D40 §1). */
function ServerAccessNote(): JSX.Element | null {
  if (!ASKS_SERVER_ACCESS) return null;
  return (
    <p class="caption muted">
      Firefox will ask to let PWM reach its server. It needs that to sign you in
      and sync your vault.
    </p>
  );
}

/** Shown once: the Secret Key and the Emergency Kit. */
function SecretKey({
  secretKey,
  onDone,
}: {
  secretKey: string;
  onDone: () => void;
}): JSX.Element {
  const [copied, setCopied] = useState(false);
  const [saved, setSaved] = useState(false);

  /** The same PDF kit as every other platform, drawn by the core (D42). */
  const download = async (): Promise<void> => {
    const { pdf } = await send({ kind: "emergencyKit" });
    const url = URL.createObjectURL(
      new Blob([fromBase64(pdf)], { type: "application/pdf" }),
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = "Emergency Kit.pdf";
    link.click();
    URL.revokeObjectURL(url);
    setSaved(true);
  };

  const copy = async (): Promise<void> => {
    await send({ kind: "copySecretKey" });
    setCopied(true);
  };

  return (
    <div class="stack">
      <h2>Save your Secret Key</h2>
      <p>
        You need it with your master password to sign in on a new device. It is
        shown only now.
      </p>
      <code class="secret">{secretKey}</code>
      <div class="row">
        <button onClick={() => void download()}>Download Emergency Kit</button>
        <button onClick={() => void copy()}>
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <p class="caption muted">
        {copied ? "The clipboard clears in 30 seconds." : " "}
      </p>
      <button class="primary" disabled={!saved && !copied} onClick={onDone}>
        I saved it
      </button>
    </div>
  );
}

/**
 * "Autofill didn't work here?" (D38 §5): pick what happened, look at what
 * would be sent (a summary, and the exact JSON on request), then Send or
 * Cancel. Nothing leaves before Send.
 */
import { useState } from "preact/hooks";
import type { JSX } from "preact";
import { HOSTED_SERVER } from "./config.ts";
import type { ReportPreview, ReportProblem } from "./protocol.ts";
import { describe, send } from "./api.ts";
import { Notice } from "./ui.tsx";

const PROBLEMS: { value: ReportProblem; text: string }[] = [
  { value: "nothing_filled", text: "Nothing was filled" },
  { value: "wrong_field", text: "The wrong field was filled" },
  { value: "no_save_offer", text: "It didn't offer to save" },
  { value: "icon_in_the_way", text: "The icon was in the way" },
  { value: "other", text: "Something else" },
];

const KINDS: Record<string, string> = {
  username: "username",
  email: "email",
  "current-password": "password",
  "new-password": "new password",
  "one-time-code": "one-time code",
  ignore: "other",
};

export function ReportLink({ onOpen }: { onOpen: () => void }): JSX.Element {
  return (
    <button class="link caption" onClick={onOpen}>
      Autofill didn't work here?
    </button>
  );
}

export function Report({ onDone }: { onDone: () => void }): JSX.Element {
  const [problem, setProblem] = useState<ReportProblem>("nothing_filled");
  const [preview, setPreview] = useState<ReportPreview | null>(null);
  const [details, setDetails] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{
    kind: "danger" | "info";
    text: string;
  } | null>(null);

  const look = async (): Promise<void> => {
    setBusy(true);
    setMessage(null);
    try {
      setPreview(await send({ kind: "reportPreview", problem }));
    } catch (err) {
      setMessage({ kind: "danger", text: describe(err) });
    } finally {
      setBusy(false);
    }
  };

  const submit = async (): Promise<void> => {
    setBusy(true);
    try {
      await send({ kind: "reportSend" });
      setPreview(null);
      setMessage({ kind: "info", text: "Thanks. We look at every report." });
    } catch (err) {
      setMessage({ kind: "danger", text: describe(err) });
    } finally {
      setBusy(false);
    }
  };

  const host = new URL(HOSTED_SERVER).host;

  return (
    <main class="stack">
      <button class="link" onClick={onDone}>
        ← Back
      </button>
      <h2>Autofill didn't work here?</h2>
      {preview === null ? (
        <>
          <fieldset class="stack">
            <legend class="label">What happened?</legend>
            {PROBLEMS.map((p) => (
              <label key={p.value} class="row">
                <input
                  type="radio"
                  name="problem"
                  checked={problem === p.value}
                  onChange={() => {
                    setProblem(p.value);
                  }}
                />
                {p.text}
              </label>
            ))}
          </fieldset>
          <button class="primary" disabled={busy} onClick={() => void look()}>
            {busy ? "Looking at the page…" : "Next"}
          </button>
        </>
      ) : (
        <section class="stack">
          <p>
            This goes to {host}, without your account or anything you typed:
          </p>
          <ul class="caption">
            <li>The site: {preview.site}</li>
            <li>
              {String(preview.forms)} form(s) with {String(preview.fields)}{" "}
              field(s):{" "}
              {preview.kinds.map((k) => KINDS[k] ?? k).join(", ") || "none"}
            </li>
            <li>Field names and labels, with emails and numbers removed</li>
            <li>The extension's and browser's versions</li>
          </ul>
          <button
            class="link caption"
            aria-expanded={details}
            onClick={() => {
              setDetails(!details);
            }}
          >
            {details ? "Hide everything" : "Show everything"}
          </button>
          {details && (
            <pre class="report-json">
              {JSON.stringify(JSON.parse(preview.json), null, 2)}
            </pre>
          )}
          <div class="row">
            <button
              class="primary"
              disabled={busy}
              onClick={() => void submit()}
            >
              {busy ? "Sending…" : "Send"}
            </button>
            <button
              disabled={busy}
              onClick={() => {
                setPreview(null);
                onDone();
              }}
            >
              Cancel
            </button>
          </div>
        </section>
      )}
      <div class="slot">
        {message && <Notice kind={message.kind}>{message.text}</Notice>}
      </div>
    </main>
  );
}

/**
 * The manage page (D44): password health, devices, import (D51) and export,
 * in a tab of its own. Like the popup it only sends messages to the background; it
 * receives an export only when the user asks for one with the master
 * password, and drops it once the download starts.
 */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import type {
  ApprovalView,
  DeviceView,
  ExportFormat,
  HealthItem,
  HealthView,
  ImportDetected,
  ImportField,
  ImportMapping,
  ImportPreviewView,
  Status,
} from "./protocol.ts";
import { fromBase64, toBase64 } from "./bytes.ts";
import { RequestError, describe, describeCode, send } from "./api.ts";
import { Unlock } from "./App.tsx";
import { host } from "./host.ts";
import { Field, Notice } from "./ui.tsx";

/** The leak check's origin, asked for from a click (D44 §2). */
const LEAK_CHECK_ORIGIN = "https://api.pwnedpasswords.com/*";

type Tab = "health" | "devices" | "import" | "export";

const TABS: { id: Tab; label: string }[] = [
  { id: "health", label: "Password health" },
  { id: "devices", label: "Devices" },
  { id: "import", label: "Import" },
  { id: "export", label: "Export" },
];

export function Manage(): JSX.Element {
  const [status, setStatus] = useState<Status | null>(null);
  const [tab, setTab] = useState<Tab>(
    (["health", "devices", "import", "export"] as const).find(
      (t) => `#${t}` === location.hash,
    ) ?? "health",
  );

  const refresh = (): void => {
    send({ kind: "status" })
      .then(setStatus)
      .catch(() => {
        setStatus({ state: "signedOut" });
      });
  };
  useEffect(refresh, []);

  if (status === null) return <main class="stack" aria-busy="true" />;
  if (status.state === "signedOut") {
    return (
      <main class="stack">
        <h1>PWM</h1>
        <p>
          {host().browser
            ? "Sign in from the extension's toolbar button first."
            : "Sign in first."}
        </p>
      </main>
    );
  }
  if (status.state === "locked")
    return <Unlock email={status.email} onDone={refresh} />;

  return (
    <main class="stack">
      {host().closeManage && (
        <button
          class="link"
          onClick={() => {
            host().closeManage?.();
          }}
        >
          ← Back to the vault
        </button>
      )}
      <h1>PWM</h1>
      <p class="muted">{status.email}</p>
      <div class="tabs" role="tablist">
        {TABS.map((t) => (
          <button
            key={t.id}
            role="tab"
            id={`tab-${t.id}`}
            aria-selected={tab === t.id}
            onClick={() => {
              setTab(t.id);
              history.replaceState(null, "", `#${t.id}`);
            }}
          >
            {t.label}
          </button>
        ))}
      </div>
      <section role="tabpanel" aria-labelledby={`tab-${tab}`} class="stack">
        {tab === "health" && <Health onLocked={refresh} />}
        {tab === "devices" && (
          <>
            <Approvals onLocked={refresh} />
            <Devices onLocked={refresh} />
          </>
        )}
        {tab === "import" && <Import onLocked={refresh} />}
        {tab === "export" && <Export onLocked={refresh} />}
      </section>
    </main>
  );
}

/** Runs a request; a locked vault sends the page back to Unlock. */
function useRequest(onLocked: () => void) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async <T,>(work: () => Promise<T>): Promise<T | undefined> => {
    setBusy(true);
    setError(null);
    try {
      return await work();
    } catch (err) {
      if (err instanceof RequestError && err.code === "locked") onLocked();
      else setError(describe(err));
      return undefined;
    } finally {
      setBusy(false);
    }
  };
  return { error, busy, run };
}

// --- Health ---

function Health({ onLocked }: { onLocked: () => void }): JSX.Element {
  const [health, setHealth] = useState<HealthView | null>(null);
  const { error, busy, run } = useRequest(onLocked);
  const [denied, setDenied] = useState(false);

  useEffect(() => {
    void run(() => send({ kind: "health" })).then((h) => {
      if (h) setHealth(h);
    });
  }, []);

  if (health === null) return <p class="muted">{error ?? "Loading…"}</p>;

  const check = async (): Promise<void> => {
    // The permission prompt needs this click (D44 §2).
    const granted = await host().requestAccess({
      origins: [LEAK_CHECK_ORIGIN],
    });
    setDenied(!granted);
    if (!granted) return;
    const h = await run(() => send({ kind: "checkLeaks" }));
    if (h) setHealth(h);
  };

  const toggle = async (on: boolean): Promise<void> => {
    const value = await run(() => send({ kind: "setLeakCheck", on }));
    if (value !== undefined) {
      const h = await run(() => send({ kind: "health" }));
      if (h) setHealth(h);
    }
  };

  const clean =
    health.weak.length === 0 &&
    health.reused.length === 0 &&
    (health.leaked?.length ?? 0) === 0;

  return (
    <div class="stack">
      <label class="row">
        <input
          type="checkbox"
          checked={health.leakCheck}
          onChange={(e) => void toggle(e.currentTarget.checked)}
        />
        Check for leaked passwords
      </label>
      <p class="caption muted">
        Checks your passwords against known leaks without sending them: only the
        first 5 characters of a hash leave your device.
      </p>
      {health.leakCheck ? (
        <button disabled={busy} onClick={() => void check()}>
          {busy ? "Checking…" : "Check for leaks"}
        </button>
      ) : (
        <Notice kind="info">
          Leak check is off. Turn it on to check without sending your passwords.
        </Notice>
      )}
      {denied && (
        <Notice kind="danger">{describeCode("no_leak_access")}</Notice>
      )}
      {error && <Notice kind="danger">{error}</Notice>}

      {health.leaked !== null && health.leaked.length > 0 && (
        <Group
          title="Leaked"
          note="These passwords appear in known data breaches. Change them first."
          items={health.leaked.map((i) => ({
            item: i,
            detail: `Seen ${i.count.toLocaleString()} times in leaks`,
          }))}
        />
      )}
      {health.weak.length > 0 && (
        <Group
          title="Weak"
          items={health.weak.map((i) => ({ item: i, detail: "Easy to guess" }))}
        />
      )}
      {health.reused.length > 0 && (
        <Group
          title="Reused"
          note="A leak on one site would open the others. Give each its own password."
          items={health.reused.flatMap((group) =>
            group.map((i) => ({
              item: i,
              detail: `Same password as ${String(group.length - 1)} other`,
            })),
          )}
        />
      )}
      {clean && (
        <p id="health-clean">
          No problems found
          {health.leaked === null ? " (leaks not checked yet)." : "."}
        </p>
      )}
    </div>
  );
}

function Group({
  title,
  note,
  items,
}: {
  title: string;
  note?: string;
  items: { item: HealthItem; detail: string }[];
}): JSX.Element {
  return (
    <section class="stack">
      <h2>{title}</h2>
      {note !== undefined && <p class="caption muted">{note}</p>}
      <ul class="list">
        {items.map(({ item, detail }) => (
          <li key={`${title}-${item.itemId}`}>
            <span class="stack">
              <strong>{item.title || item.username}</strong>
              <span class="caption muted">{detail}</span>
            </span>
            <ChangeLink item={item} />
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Opens the site; the extension offers to update the login there (D44 §4). */
function ChangeLink({ item }: { item: HealthItem }): JSX.Element {
  const url = URL.canParse(item.url) ? new URL(item.url) : null;
  if (url === null || !/^https?:$/.test(url.protocol)) {
    return (
      <span class="caption muted">
        Change it on the site, then save it here
      </span>
    );
  }
  return (
    <a href={url.href} target="_blank" rel="noreferrer noopener">
      Change
    </a>
  );
}

// --- Approving a new device (D46) ---

function Approvals({ onLocked }: { onLocked: () => void }): JSX.Element {
  const [requests, setRequests] = useState<ApprovalView[] | null>(null);
  const [approving, setApproving] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const { error, busy, run } = useRequest(onLocked);
  const current = requests?.find((r) => r.id === approving);

  useEffect(() => {
    void run(() => send({ kind: "approvals" })).then((r) => {
      if (r) setRequests(r);
    });
  }, []);

  // After accepting: read the list until the new device revealed its key.
  useEffect(() => {
    if (approving === null || current?.code) return;
    const timer = setTimeout(() => {
      void run(() => send({ kind: "approvals" })).then((r) => {
        if (!r) return;
        setRequests(r);
        if (!r.some((x) => x.id === approving)) setApproving(null);
      });
    }, 2000);
    return () => {
      clearTimeout(timer);
    };
  }, [approving, requests]);

  if (requests === null || (requests.length === 0 && !done)) return <></>;

  const accept = async (id: string): Promise<void> => {
    setDone(false);
    const r = await run(() => send({ kind: "acceptApproval", id }));
    if (r) {
      setRequests(r);
      setApproving(id);
    }
  };

  const codesMatch = async (id: string): Promise<void> => {
    const r = await run(() => send({ kind: "completeApproval", id }));
    if (r) {
      setRequests(r);
      setApproving(null);
      setDone(true);
    }
  };

  const deny = async (id: string): Promise<void> => {
    const r = await run(() => send({ kind: "denyApproval", id }));
    if (r) setRequests(r);
    if (approving === id) setApproving(null);
  };

  return (
    <section class="stack card" id="approvals">
      <h2>Approve a new device</h2>
      {done && (
        <Notice kind="info">
          Approved. Enter your master password on the new device.
        </Notice>
      )}
      {approving !== null && current ? (
        current.code ? (
          <div class="stack">
            <p>Does the new device show this code?</p>
            <p class="secret" id="approval-code">
              {current.code}
            </p>
            <div class="row">
              <button
                class="primary"
                id="codes-match"
                disabled={busy}
                onClick={() => void codesMatch(current.id)}
              >
                Codes match
              </button>
              <button class="danger" onClick={() => void deny(current.id)}>
                Codes don't match
              </button>
            </div>
            <p class="caption muted">
              Only approve a device you are setting up yourself. It still needs
              your master password.
            </p>
          </div>
        ) : (
          <p class="muted">Waiting for the new device…</p>
        )
      ) : (
        <ul class="list">
          {requests.map((request) => (
            <li key={request.id} class="approval-request">
              <span class="stack">
                <strong>
                  {PLATFORMS[request.platform] ?? request.platform}
                </strong>
                <span class="caption muted">
                  Asked {new Date(request.createdAt).toLocaleTimeString()}
                </span>
              </span>
              <span class="row">
                <button
                  class="primary"
                  disabled={busy}
                  onClick={() => void accept(request.id)}
                >
                  Approve
                </button>
                <button class="danger" onClick={() => void deny(request.id)}>
                  Deny
                </button>
              </span>
            </li>
          ))}
        </ul>
      )}
      {error && <Notice kind="danger">{error}</Notice>}
    </section>
  );
}

// --- Devices ---

const PLATFORMS: Record<string, string> = {
  ios: "iOS",
  android: "Android",
  macos: "macOS",
  windows: "Windows",
  linux: "Linux",
  extension: "Browser extension",
};

function Devices({ onLocked }: { onLocked: () => void }): JSX.Element {
  const [devices, setDevices] = useState<DeviceView[] | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [name, setName] = useState("");
  const { error, busy, run } = useRequest(onLocked);

  useEffect(() => {
    void run(() => send({ kind: "devices" })).then((d) => {
      if (d) setDevices(d);
    });
  }, []);

  if (devices === null) return <p class="muted">{error ?? "Loading…"}</p>;

  const rename = async (deviceId: string): Promise<void> => {
    const d = await run(() => send({ kind: "renameDevice", deviceId, name }));
    if (d) {
      setDevices(d);
      setRenaming(null);
    }
  };

  const revoke = async (device: DeviceView): Promise<void> => {
    const label = device.name ?? "this device";
    if (
      !confirm(
        `Sign out ${label}? It loses access at once and deletes its copy of your vault the next time it connects.`,
      )
    )
      return;
    const d = await run(() =>
      send({ kind: "revokeDevice", deviceId: device.id }),
    );
    if (d) setDevices(d);
  };

  return (
    <div class="stack">
      <ul class="list">
        {devices.map((device) => (
          <li
            key={device.id}
            id={device.current ? "device-current" : undefined}
          >
            <span class="stack">
              <strong>
                {device.name ?? "Unknown device"}
                {device.current && (
                  <span class="caption muted"> · This device</span>
                )}
              </strong>
              <span class="caption muted">
                {PLATFORMS[device.platform] ?? device.platform} ·{" "}
                {device.revokedAt === null
                  ? `Last used ${new Date(device.lastSeenAt).toLocaleString()}`
                  : `Signed out ${new Date(device.revokedAt).toLocaleDateString()}`}
              </span>
              {renaming === device.id && (
                <form
                  class="row"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void rename(device.id);
                  }}
                >
                  <Field label="Name" value={name} onInput={setName} />
                  <button class="primary" disabled={busy || name.trim() === ""}>
                    Save
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      setRenaming(null);
                    }}
                  >
                    Cancel
                  </button>
                </form>
              )}
            </span>
            {device.revokedAt === null && renaming !== device.id && (
              <span class="row">
                <button
                  onClick={() => {
                    setName(device.name ?? "");
                    setRenaming(device.id);
                  }}
                >
                  Rename
                </button>
                {!device.current && (
                  <button
                    class="danger"
                    disabled={busy}
                    onClick={() => void revoke(device)}
                  >
                    Sign out
                  </button>
                )}
              </span>
            )}
          </li>
        ))}
      </ul>
      <p class="caption muted">
        Only your devices can read their names. To leave this device, use Sign
        out in {host().browser ? "the extension's settings" : "Settings"}.
      </p>
      {error && <Notice kind="danger">{error}</Notice>}
    </div>
  );
}

// --- Export ---

const FORMAT_NOTES: Record<ExportFormat, string> = {
  pwmx: "Opens with the export password alone, without your account. Every item, folder and history entry.",
  json: "Every item, folder and history entry, readable by anyone who has the file.",
  csv: "Logins, notes, cards and identities as a spreadsheet, readable by anyone who has the file.",
};

// --- Import (D51 §9) ---

/** The page's own limit: the file must fit in one extension message. */
const MAX_IMPORT_BYTES = 32 * 1024 * 1024;

const IMPORT_FORMATS: { id: string; label: string }[] = [
  { id: "bitwarden", label: "Bitwarden (.json)" },
  { id: "1password", label: "1Password (.1pux)" },
  { id: "keeper", label: "Keeper (.json)" },
  { id: "keepass", label: "KeePass (.kdbx)" },
  { id: "nordpass", label: "NordPass (.csv)" },
  { id: "protonpass", label: "Proton Pass (.zip or .json)" },
  { id: "dashlane", label: "Dashlane (.zip, .csv or .json)" },
  { id: "lastpass", label: "LastPass (.csv)" },
  { id: "csv", label: "Other CSV, such as a browser's" },
  { id: "pwm", label: "PWM export (.pwmx, .json or .csv)" },
];

const MAPPED_FIELDS: { id: ImportField; label: string }[] = [
  { id: "title", label: "Title" },
  { id: "url", label: "Website" },
  { id: "username", label: "Username" },
  { id: "password", label: "Password" },
  { id: "notes", label: "Notes" },
  { id: "totp", label: "One-time code" },
  { id: "folder", label: "Folder" },
];

const LEFT_OUT: Record<string, string> = {
  attachments: "attachments and documents",
  ssh_keys: "SSH keys",
  aliases: "email aliases",
  passkeys: "passkeys this export can't carry",
  unknown_types: "items of a type PWM doesn't have",
  empty: "empty rows",
  too_large: "items too large to sync",
  duplicates: "items already in your vault",
};

function counts(p: ImportPreviewView): string {
  const parts = [
    [p.logins, "login", "logins"],
    [p.notes, "note", "notes"],
    [p.cards, "card", "cards"],
    [p.identities, "identity", "identities"],
    [p.folders, "folder", "folders"],
  ] as const;
  const listed = parts
    .filter(([n]) => n > 0)
    .map(([n, one, many]) => `${String(n)} ${n === 1 ? one : many}`);
  return listed.length > 0 ? listed.join(", ") : "nothing new";
}

function Import({ onLocked }: { onLocked: () => void }): JSX.Element {
  const [file, setFile] = useState<{ name: string; data: string } | null>(null);
  const [detected, setDetected] = useState<ImportDetected | null>(null);
  const [format, setFormat] = useState("");
  const [password, setPassword] = useState("");
  /** A KeePass key file, as base64. */
  const [keyFile, setKeyFile] = useState<{ name: string; data: string } | null>(
    null,
  );
  const [columns, setColumns] = useState<ImportMapping>({});
  const [preview, setPreview] = useState<ImportPreviewView | null>(null);
  const [done, setDone] = useState<ImportPreviewView | null>(null);
  /** The imported file had no password: it holds passwords in plain text. */
  const [plain, setPlain] = useState(false);
  const [pickError, setPickError] = useState<string | null>(null);
  const { error, busy, run } = useRequest(onLocked);

  const reset = (): void => {
    setPreview(null);
    setDone(null);
  };

  const pick = async (input: HTMLInputElement): Promise<void> => {
    const picked = input.files?.[0];
    input.value = "";
    setPickError(null);
    reset();
    setFile(null);
    setDetected(null);
    setPassword("");
    setKeyFile(null);
    if (!picked) return;
    if (picked.size > MAX_IMPORT_BYTES) {
      setPickError(describeCode("import_too_large"));
      return;
    }
    // Read into memory only; the file is never copied anywhere.
    const data = toBase64(new Uint8Array(await picked.arrayBuffer()));
    const found = await run(() =>
      send({ kind: "importDetect", name: picked.name, file: data }),
    );
    if (!found) return;
    setFile({ name: picked.name, data });
    setDetected(found);
    setFormat(found.format ?? "");
    setColumns(found.columns?.suggested ?? {});
  };

  const check = async (event: Event): Promise<void> => {
    event.preventDefault();
    if (!file) return;
    reset();
    const result = await run(() =>
      send({
        kind: "importPrepare",
        file: file.data,
        format,
        password,
        keyFile: format === "keepass" ? (keyFile?.data ?? "") : "",
        columns: format === "csv" ? columns : null,
      }),
    );
    if (result) setPreview(result);
  };

  const finish = async (): Promise<void> => {
    const result = await run(() => send({ kind: "importFinish" }));
    if (!result) return;
    setDone(result);
    setPlain(password === "" && keyFile === null);
    setPreview(null);
    setFile(null);
    setPassword("");
    setKeyFile(null);
  };

  const pickKey = async (input: HTMLInputElement): Promise<void> => {
    const picked = input.files?.[0];
    input.value = "";
    reset();
    if (!picked || picked.size > 1024 * 1024) {
      setKeyFile(null);
      return;
    }
    setKeyFile({
      name: picked.name,
      data: toBase64(new Uint8Array(await picked.arrayBuffer())),
    });
  };

  const headers = detected?.columns?.headers ?? [];
  const leftOut = (preview ?? done)?.leftOut ?? [];

  return (
    <div class="stack">
      <p>
        Bring in logins, cards, notes and folders from another password manager
        or a PWM export. The file stays on this computer.
      </p>
      <label class="field">
        <span class="label">Export file</span>
        <input
          id="import-file"
          type="file"
          accept=".json,.csv,.zip,.1pux,.pwmx,.kdbx"
          onChange={(e) => void pick(e.currentTarget)}
        />
      </label>
      {file && (
        <form class="stack" onSubmit={(e) => void check(e)}>
          <label class="field">
            <span class="label">From</span>
            <select
              id="import-format"
              value={format}
              onChange={(e) => {
                setFormat(e.currentTarget.value);
                reset();
              }}
            >
              <option value="" disabled>
                Choose where this file is from
              </option>
              {IMPORT_FORMATS.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.label}
                </option>
              ))}
            </select>
            {detected?.format === null && (
              <span class="caption muted">
                PWM couldn't tell which manager made this file.
              </span>
            )}
          </label>
          {(detected?.needsPassword === true ||
            format === "bitwarden" ||
            format === "keepass" ||
            format === "pwm") && (
            <Field
              label="Export password"
              type="password"
              value={password}
              onInput={setPassword}
              autoComplete="off"
              hint="Only if the export was made with a password."
            />
          )}
          {format === "keepass" && (
            <label class="field">
              <span class="label">Key file</span>
              <input
                id="import-key-file"
                type="file"
                onChange={(e) => void pickKey(e.currentTarget)}
              />
              <span class="caption muted">
                {keyFile
                  ? `Using ${keyFile.name}.`
                  : "Only if the database needs one."}
              </span>
            </label>
          )}
          {format === "csv" && headers.length > 0 && (
            <fieldset class="stack">
              <legend>Which column holds what</legend>
              {MAPPED_FIELDS.map((f) => (
                <label class="field" key={f.id}>
                  <span class="label">{f.label}</span>
                  <select
                    id={`import-column-${f.id}`}
                    value={String(columns[f.id] ?? "")}
                    onChange={(e) => {
                      const value = e.currentTarget.value;
                      setColumns({
                        ...columns,
                        [f.id]: value === "" ? null : Number(value),
                      });
                      reset();
                    }}
                  >
                    <option value="">None</option>
                    {headers.map((h, i) => (
                      <option key={i} value={String(i)}>
                        {h === "" ? `Column ${String(i + 1)}` : h}
                      </option>
                    ))}
                  </select>
                </label>
              ))}
            </fieldset>
          )}
          <button id="import-check" disabled={busy || format === ""}>
            {busy && !preview ? "Reading…" : "Check file"}
          </button>
        </form>
      )}
      <div class="slot">
        {(pickError ?? error) && (
          <Notice kind="danger">{pickError ?? error}</Notice>
        )}
      </div>
      {preview && (
        <section class="stack card" id="import-preview">
          <p>Ready to import {counts(preview)}.</p>
          {preview.trashed > 0 && (
            <p class="caption muted">
              {String(preview.trashed)} of them come in the trash, as they were
              archived or deleted there.
            </p>
          )}
          {leftOut.length > 0 && (
            <ul class="caption muted">
              {leftOut.map((l) => (
                <li key={l.reason}>
                  Left out: {String(l.count)} {LEFT_OUT[l.reason] ?? l.reason}
                </li>
              ))}
            </ul>
          )}
          <button
            class="primary"
            id="import"
            disabled={busy}
            onClick={() => void finish()}
          >
            {busy ? "Importing…" : "Import"}
          </button>
        </section>
      )}
      {done && (
        <section class="stack" id="import-done">
          <Notice kind="info">Imported {counts(done)}.</Notice>
          {leftOut.length > 0 && (
            <ul class="caption muted">
              {leftOut.map((l) => (
                <li key={l.reason}>
                  Left out: {String(l.count)} {LEFT_OUT[l.reason] ?? l.reason}
                </li>
              ))}
            </ul>
          )}
          {plain && (
            <Notice kind="danger">
              The export file holds your passwords in plain text. Delete it now,
              and empty the trash.
            </Notice>
          )}
        </section>
      )}
    </div>
  );
}

function Export({ onLocked }: { onLocked: () => void }): JSX.Element {
  const [format, setFormat] = useState<ExportFormat>("pwmx");
  const [masterPassword, setMasterPassword] = useState("");
  const [exportPassword, setExportPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [understood, setUnderstood] = useState(false);
  const [score, setScore] = useState(0);
  const [saved, setSaved] = useState<string | null>(null);
  const { error, busy, run } = useRequest(onLocked);

  useEffect(() => {
    if (exportPassword === "") {
      setScore(0);
      return;
    }
    void send({ kind: "score", password: exportPassword, email: "" }).then(
      setScore,
    );
  }, [exportPassword]);

  const ready =
    masterPassword !== "" &&
    (format === "pwmx"
      ? score >= 3 && exportPassword === confirmPassword
      : understood);

  const submit = async (event: Event): Promise<void> => {
    event.preventDefault();
    setSaved(null);
    const result = await run(() =>
      send({ kind: "exportVault", masterPassword, format, exportPassword }),
    );
    setMasterPassword("");
    if (!result) return;
    // Saved as a download, like the Emergency Kit; nothing is kept here.
    const url = URL.createObjectURL(
      new Blob([fromBase64(result.file)], {
        type: "application/octet-stream",
      }),
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = result.name;
    link.click();
    URL.revokeObjectURL(url);
    setExportPassword("");
    setConfirmPassword("");
    setUnderstood(false);
    setSaved(result.name);
  };

  return (
    <form class="stack" onSubmit={(e) => void submit(e)}>
      <label class="field">
        <span class="label">Format</span>
        <select
          id="export-format"
          value={format}
          onChange={(e) => {
            setFormat(e.currentTarget.value as ExportFormat);
            setUnderstood(false);
          }}
        >
          <option value="pwmx">Encrypted (.pwmx)</option>
          <option value="json">JSON, not encrypted</option>
          <option value="csv">CSV, not encrypted</option>
        </select>
        <span class="caption muted">{FORMAT_NOTES[format]}</span>
      </label>
      <Field
        label="Master password"
        type="password"
        value={masterPassword}
        onInput={setMasterPassword}
        autoComplete="current-password"
        hint="Every export asks for it again."
      />
      {format === "pwmx" ? (
        <>
          <Field
            label="Export password"
            type="password"
            value={exportPassword}
            onInput={setExportPassword}
            autoComplete="new-password"
            hint={
              exportPassword === "" || score >= 3
                ? "Needed to open the file."
                : "Too easy to guess."
            }
          />
          <Field
            label="Export password again"
            type="password"
            value={confirmPassword}
            onInput={setConfirmPassword}
            autoComplete="new-password"
          />
        </>
      ) : (
        <label class="row">
          <input
            id="export-understood"
            type="checkbox"
            checked={understood}
            onChange={(e) => {
              setUnderstood(e.currentTarget.checked);
            }}
          />
          I understand that anyone who gets this file can read all my passwords.
        </label>
      )}
      <div class="slot">
        {error && <Notice kind="danger">{error}</Notice>}
        {saved && <Notice kind="info">Saved {saved}.</Notice>}
      </div>
      <button class="primary" id="export" disabled={busy || !ready}>
        {busy ? "Exporting…" : "Export"}
      </button>
      <p class="caption muted">Export works on every plan.</p>
    </form>
  );
}

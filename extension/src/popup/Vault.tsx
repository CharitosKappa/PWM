/** The unlocked popup: search, item list and detail, copy, generator (D34). No editing yet. */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import type { CopyField, ItemSummary, Status } from "../shared/messages.ts";
import { describe, describeCode, send } from "./api.ts";
import { Notice } from "./ui.tsx";

type Unlocked = Extract<Status, { state: "unlocked" }>;

export function Vault({
  status,
  onStatus,
  onSettings,
}: {
  status: Unlocked;
  onStatus: (status: Status) => void;
  onSettings: () => void;
}): JSX.Element {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<ItemSummary[]>([]);
  const [selected, setSelected] = useState<ItemSummary | null>(null);
  const [generator, setGenerator] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    send({ kind: "items", query })
      .then((value) => {
        if (current) setItems(value);
      })
      .catch((err: unknown) => {
        if (current) setToast(describe(err));
      });
    return () => {
      current = false;
    };
  }, [query, status.syncedAt]);

  useEffect(() => {
    if (toast === null) return;
    const timer = setTimeout(() => {
      setToast(null);
    }, 3000);
    return () => {
      clearTimeout(timer);
    };
  }, [toast]);

  const copy = async (item: ItemSummary, field: CopyField): Promise<void> => {
    try {
      await send({
        kind: "copy",
        vaultId: item.vaultId,
        itemId: item.itemId,
        field,
      });
      setToast("Copied. The clipboard clears in 30 seconds.");
    } catch (err) {
      setToast(describe(err));
    }
  };

  const lock = async (): Promise<void> => {
    await send({ kind: "lock" });
    onStatus(await send({ kind: "status" }));
  };

  const sync = async (): Promise<void> => {
    onStatus(await send({ kind: "sync" }));
  };

  return (
    <main class="stack">
      <header class="row">
        <input
          class="search"
          type="search"
          placeholder="Search"
          aria-label="Search"
          value={query}
          // The popup opens to search.
          autoFocus
          onInput={(event) => {
            setQuery(event.currentTarget.value);
            setSelected(null);
          }}
        />
        <button
          aria-pressed={generator}
          onClick={() => {
            setGenerator(!generator);
          }}
        >
          Generate
        </button>
        <button onClick={onSettings}>Settings</button>
        <button onClick={() => void lock()}>Lock</button>
      </header>
      <SyncLine status={status} onSync={() => void sync()} />
      {generator && <Generator onToast={setToast} />}
      {selected ? (
        <Detail
          item={selected}
          onBack={() => {
            setSelected(null);
          }}
          onCopy={(field) => void copy(selected, field)}
        />
      ) : (
        <ItemList
          items={items}
          query={query}
          onSelect={setSelected}
          onCopy={(item, field) => void copy(item, field)}
        />
      )}
      {/* Floats over the list, so nothing moves under the cursor. */}
      <div class="toast">
        {toast !== null && <Notice kind="info">{toast}</Notice>}
      </div>
    </main>
  );
}

function SyncLine({
  status,
  onSync,
}: {
  status: Unlocked;
  onSync: () => void;
}): JSX.Element {
  let text = "Not synced yet";
  if (status.syncError !== null) text = describeCode(status.syncError);
  else if (status.syncedAt !== null)
    text = `Synced ${new Date(status.syncedAt).toLocaleTimeString()}`;
  return (
    <div class="row caption muted">
      <span>{status.email}</span>
      <span class="grow" />
      <span>{text}</span>
      <button class="link" onClick={onSync}>
        Sync
      </button>
    </div>
  );
}

function ItemList({
  items,
  query,
  onSelect,
  onCopy,
}: {
  items: ItemSummary[];
  query: string;
  onSelect: (item: ItemSummary) => void;
  onCopy: (item: ItemSummary, field: CopyField) => void;
}): JSX.Element {
  if (items.length === 0) {
    return (
      <p class="muted">
        {query === ""
          ? "No items yet. Save one from a sign-in page or the app."
          : "Nothing matches."}
      </p>
    );
  }
  return (
    <ul class="list">
      {items.map((item) => (
        <li key={`${item.vaultId}/${item.itemId}`} class="row">
          <button
            class="item grow"
            onClick={() => {
              onSelect(item);
            }}
          >
            <span class="strong">{item.title || "Untitled"}</span>
            <span class="caption muted">{item.username || item.url}</span>
          </button>
          {item.hasPassword && (
            <button
              aria-label={`Copy password for ${item.title}`}
              onClick={() => {
                onCopy(item, "password");
              }}
            >
              Copy
            </button>
          )}
        </li>
      ))}
    </ul>
  );
}

function Detail({
  item,
  onBack,
  onCopy,
}: {
  item: ItemSummary;
  onBack: () => void;
  onCopy: (field: CopyField) => void;
}): JSX.Element {
  return (
    <section class="stack">
      <button class="link" onClick={onBack}>
        ← All items
      </button>
      <h2>{item.title || "Untitled"}</h2>
      {item.url !== "" && <p class="caption muted">{item.url}</p>}
      {item.username !== "" && (
        <div class="row">
          <span class="grow">{item.username}</span>
          <button
            onClick={() => {
              onCopy("username");
            }}
          >
            Copy username
          </button>
        </div>
      )}
      {item.hasPassword && (
        <div class="row">
          <span class="grow mono">••••••••••</span>
          <button
            onClick={() => {
              onCopy("password");
            }}
          >
            Copy password
          </button>
        </div>
      )}
      {item.hasCode && (
        <div class="row">
          <span class="grow">One-time code</span>
          <button
            onClick={() => {
              onCopy("code");
            }}
          >
            Copy code
          </button>
        </div>
      )}
    </section>
  );
}

function Generator({
  onToast,
}: {
  onToast: (text: string) => void;
}): JSX.Element {
  const [mode, setMode] = useState<"password" | "passphrase">("password");
  const [value, setValue] = useState("");
  const [round, setRound] = useState(0);

  useEffect(() => {
    send({ kind: "generate", mode })
      .then(setValue)
      .catch((err: unknown) => {
        onToast(describe(err));
      });
  }, [mode, round, onToast]);

  const copy = async (): Promise<void> => {
    try {
      await send({ kind: "copyText", text: value });
      onToast("Copied. The clipboard clears in 30 seconds.");
    } catch (err) {
      onToast(describe(err));
    }
  };

  return (
    <section class="stack card">
      <div class="tabs" role="tablist">
        <button
          role="tab"
          aria-selected={mode === "password"}
          onClick={() => {
            setMode("password");
          }}
        >
          Password
        </button>
        <button
          role="tab"
          aria-selected={mode === "passphrase"}
          onClick={() => {
            setMode("passphrase");
          }}
        >
          Passphrase
        </button>
      </div>
      <code class="secret">{value}</code>
      <div class="row">
        <button
          onClick={() => {
            setRound(round + 1);
          }}
        >
          New
        </button>
        <button class="primary" onClick={() => void copy()}>
          Copy
        </button>
      </div>
    </section>
  );
}

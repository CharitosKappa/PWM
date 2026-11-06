/**
 * The vault in a full window (D67 §3): the sidebar's sections, the list,
 * and the item's detail or editor. Narrower than 720 pixels, one pane at a
 * time. Ctrl+N makes a new login and Ctrl+F searches, as ⌘N and ⌘F on the
 * Mac. No website icons: fetching them would tell third parties which
 * sites are in the vault (D43). The desktop shows it as its window; the
 * extension on its manage page (M4.14).
 */
import { useEffect, useRef, useState } from "preact/hooks";
import type { JSX } from "preact";
import { describe, send } from "../api.ts";
import { host } from "../host.ts";
import type { Status, VaultView } from "../protocol.ts";
import { Notice } from "../ui.tsx";
import { Generator, SyncLine } from "../Vault.tsx";
import type { Unlocked } from "../Vault.tsx";
import { ItemDetail } from "../items/ItemDetail.tsx";
import { ItemEditor } from "../items/ItemEditor.tsx";
import { ITEM_TYPES, sectionLabel, visibleRows } from "../items/model.ts";
import type { ItemType, Section } from "../items/model.ts";

interface Props {
  status: Unlocked;
  onStatus: (status: Status) => void;
  /** Where there are settings to open; the extension's are in its popup. */
  onSettings?: () => void;
  /** The manage page's sections; the host's own by default. */
  onManage?: (section?: "devices") => void;
}

type Selected = { vaultId: string; itemId: string } | null;

/** What the item pane shows. */
type ItemPane =
  | { kind: "none" }
  | { kind: "detail" }
  | { kind: "edit" }
  | { kind: "new"; type: ItemType }
  | { kind: "generator" };

/** The one pane shown on a narrow window. */
type Narrow = "sidebar" | "list" | "item";

const TYPE_LABELS: Record<string, string> = {
  login: "Login",
  card: "Card",
  identity: "Identity",
  note: "Note",
};

export function VaultWindow({
  status,
  onStatus,
  onSettings,
  onManage,
}: Props): JSX.Element {
  const manage =
    onManage ??
    ((section?: "devices") => {
      host().openManage(section);
    });
  const [vault, setVault] = useState<VaultView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [section, setSection] = useState<Section>({ kind: "all" });
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Selected>(null);
  const [pane, setPane] = useState<ItemPane>({ kind: "none" });
  const [narrow, setNarrow] = useState<Narrow>("list");
  const [toast, setToast] = useState<string | null>(null);
  const search = useRef<HTMLInputElement>(null);

  const load = (): void => {
    send({ kind: "vault" })
      .then((v) => {
        setVault(v);
        setError(null);
      })
      .catch((err: unknown) => {
        setError(describe(err));
      });
  };
  useEffect(load, []);

  useEffect(() => {
    if (toast === null) return;
    const timer = setTimeout(() => {
      setToast(null);
    }, 3000);
    return () => {
      clearTimeout(timer);
    };
  }, [toast]);

  const startNew = (type: ItemType): void => {
    setSelected(null);
    setPane({ kind: "new", type });
    setNarrow("item");
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (!e.ctrlKey || e.altKey || e.shiftKey) return;
      if (e.key === "n" || e.key === "N") {
        e.preventDefault();
        startNew("login");
      } else if (e.key === "f" || e.key === "F") {
        e.preventDefault();
        setNarrow("list");
        search.current?.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
    };
  }, []);

  const sync = (): void => {
    send({ kind: "sync" })
      .then((s) => {
        onStatus(s);
        load();
      })
      .catch((err: unknown) => {
        setToast(describe(err));
      });
  };

  const lock = (): void => {
    send({ kind: "lock" })
      .then(() => send({ kind: "status" }))
      .then(onStatus)
      .catch((err: unknown) => {
        setToast(describe(err));
      });
  };

  const choose = (next: Section): void => {
    setSection(next);
    setNarrow("list");
    if (pane.kind === "generator") setPane({ kind: "none" });
  };

  const folders = vault?.folders ?? [];
  const rows = vault ? visibleRows(vault.items, section, query) : [];
  const same = (a: Section, b: Section): boolean =>
    JSON.stringify(a) === JSON.stringify(b);
  const sectionButton = (s: Section, label: string): JSX.Element => (
    <button
      class={same(s, section) ? "nav current" : "nav"}
      aria-current={same(s, section) ? "true" : undefined}
      onClick={() => {
        choose(s);
      }}
    >
      {label}
    </button>
  );

  return (
    <div class="vault-window" data-narrow={narrow}>
      <nav class="pane sidebar stack" aria-label="Sections">
        {sectionButton({ kind: "all" }, "All items")}
        {sectionButton({ kind: "favorites" }, "Favorites")}
        {ITEM_TYPES.map((t) => (
          <span key={t.type}>
            {sectionButton({ kind: "type", type: t.type }, t.plural)}
          </span>
        ))}
        {folders.length > 0 && <span class="label">Folders</span>}
        {folders.map((f) => (
          <span key={f.itemId}>
            {sectionButton({ kind: "folder", folderId: f.itemId }, f.title)}
          </span>
        ))}
        {sectionButton({ kind: "trash" }, "Trash")}
        <span class="grow" />
        <button
          class={pane.kind === "generator" ? "nav current" : "nav"}
          onClick={() => {
            setPane({ kind: "generator" });
            setNarrow("item");
          }}
        >
          Generator
        </button>
        {onSettings && (
          <button class="nav" onClick={onSettings}>
            Settings
          </button>
        )}
        {host().manage && (
          <button
            class="nav"
            onClick={() => {
              manage();
            }}
          >
            Password health, devices and export
          </button>
        )}
        <button class="nav" onClick={lock}>
          Lock
        </button>
      </nav>

      <section class="pane list-pane stack" aria-label="Items">
        <div class="row narrow-only">
          <button
            class="link"
            onClick={() => {
              setNarrow("sidebar");
            }}
          >
            ← Sections
          </button>
        </div>
        <SyncLine status={status} onSync={sync} />
        {status.approvals > 0 && host().manage && (
          <p class="notice info row" role="status">
            <span class="grow">A new device is asking to sign in.</span>
            <button
              class="link"
              onClick={() => {
                manage("devices");
              }}
            >
              Review
            </button>
          </p>
        )}
        <div class="row">
          <h2 class="grow">{sectionLabel(section, folders)}</h2>
          <select
            aria-label="New item"
            value=""
            onChange={(e) => {
              const type = ITEM_TYPES.find(
                (t) => t.type === e.currentTarget.value,
              )?.type;
              if (type !== undefined) startNew(type);
            }}
          >
            <option value="">New…</option>
            {ITEM_TYPES.map((t) => (
              <option key={t.type} value={t.type}>
                {t.label}
              </option>
            ))}
          </select>
        </div>
        <input
          ref={search}
          class="search"
          type="search"
          placeholder="Search"
          aria-label="Search"
          value={query}
          onInput={(e) => {
            setQuery(e.currentTarget.value);
          }}
        />
        {error && <Notice kind="danger">{error}</Notice>}
        {vault && rows.length === 0 && (
          <p class="muted">
            {query.trim() !== ""
              ? "Nothing matches."
              : section.kind === "trash"
                ? "The trash is empty."
                : "No items here yet."}
          </p>
        )}
        <ul class="list rows" role="list">
          {rows.map((row) => (
            <li key={row.itemId}>
              <button
                class={
                  selected?.itemId === row.itemId
                    ? "row-button current"
                    : "row-button"
                }
                onClick={() => {
                  setSelected({ vaultId: row.vaultId, itemId: row.itemId });
                  setPane({ kind: "detail" });
                  setNarrow("item");
                }}
              >
                <span class="item-text">
                  <span class="strong">{row.title || "Untitled"}</span>
                  <span class="caption muted">{row.subtitle}</span>
                </span>
                <span class="caption muted">
                  {TYPE_LABELS[row.type] ?? row.type}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </section>

      <section class="pane item-pane stack" aria-label="Item">
        <div class="row narrow-only">
          <button
            class="link"
            onClick={() => {
              setNarrow("list");
            }}
          >
            ← Back
          </button>
        </div>
        {pane.kind === "generator" && <Generator onToast={setToast} />}
        {pane.kind === "detail" && selected && (
          <ItemDetail
            key={selected.itemId}
            vaultId={selected.vaultId}
            itemId={selected.itemId}
            folders={folders}
            onEdit={() => {
              setPane({ kind: "edit" });
            }}
            onChanged={() => {
              load();
              setSelected(null);
              setPane({ kind: "none" });
              setNarrow("list");
            }}
            onToast={setToast}
          />
        )}
        {(pane.kind === "edit" || pane.kind === "new") && (
          <ItemEditor
            key={pane.kind === "new" ? `new-${pane.type}` : selected?.itemId}
            editing={pane.kind === "edit" ? selected : null}
            type={pane.kind === "new" ? pane.type : "login"}
            folders={folders}
            onFoldersChanged={load}
            onSaved={(vaultId, itemId) => {
              load();
              setSelected({ vaultId, itemId });
              setPane({ kind: "detail" });
              setToast("Saved.");
            }}
            onCancel={() => {
              setPane(selected ? { kind: "detail" } : { kind: "none" });
              if (!selected) setNarrow("list");
            }}
          />
        )}
        {pane.kind === "none" && (
          <p class="muted">Choose an item, or press Ctrl+N for a new login.</p>
        )}
      </section>

      {/* Floats over the panes, so nothing moves under the cursor. */}
      <div class="toast" role="status">
        {toast && <p class="notice info">{toast}</p>}
      </div>
    </div>
  );
}

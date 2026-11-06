/**
 * One item, shown (D43 §2, D67 §1): every hidden value masked until Show,
 * which asks the host for that one value; Copy goes through the host's
 * clipboard. The one-time code shows with its countdown, never its secret.
 * Read-only items (a newer schema or type) have no Edit.
 */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import { describe, send } from "../api.ts";
import type { FolderView, ItemView } from "../protocol.ts";
import { Notice } from "../ui.tsx";
import { FIELD_KINDS, customFields, getIn, textAt } from "./model.ts";

interface Props {
  vaultId: string;
  itemId: string;
  folders: FolderView[];
  onEdit: () => void;
  /** The item moved to or from the trash, or was deleted. */
  onChanged: () => void;
  onToast: (text: string) => void;
}

const MASK = "••••••••";

export function ItemDetail({
  vaultId,
  itemId,
  folders,
  onEdit,
  onChanged,
  onToast,
}: Props): JSX.Element {
  const [view, setView] = useState<ItemView | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Values shown with Show, by their place; gone with the item. */
  const [shown, setShown] = useState<Record<string, string>>({});
  const [confirmDelete, setConfirmDelete] = useState(false);

  const load = (): void => {
    send({ kind: "item", vaultId, itemId })
      .then(setView)
      .catch((err: unknown) => {
        setError(describe(err));
      });
  };
  useEffect(load, [vaultId, itemId]);

  // The code changes each period: ask again when it runs out.
  useEffect(() => {
    if (!view?.code) return;
    const timer = setTimeout(load, (view.code.secondsLeft + 1) * 1000);
    return () => {
      clearTimeout(timer);
    };
  }, [view]);

  if (error) return <Notice kind="danger">{error}</Notice>;
  if (view === null) return <div class="stack" aria-busy="true" />;

  const item = view.item;
  const type = textAt(item, ["type"]);
  const trashed = getIn(item, ["trashed_at"]) != null;

  const copy = (path: string): void => {
    send({ kind: "copy", vaultId, itemId, path })
      .then(() => {
        onToast("Copied. The clipboard clears in 30 seconds.");
      })
      .catch((err: unknown) => {
        onToast(describe(err));
      });
  };

  const toggle = (path: string): void => {
    if (path in shown) {
      setShown(
        Object.fromEntries(Object.entries(shown).filter(([p]) => p !== path)),
      );
      return;
    }
    send({ kind: "reveal", vaultId, itemId, path })
      .then((value) => {
        setShown({ ...shown, [path]: value });
      })
      .catch((err: unknown) => {
        onToast(describe(err));
      });
  };

  const act = (
    kind: "trashItem" | "restoreItem" | "deleteItem",
    done: string,
  ): void => {
    send({ kind, vaultId, itemId })
      .then(() => {
        onToast(done);
        onChanged();
      })
      .catch((err: unknown) => {
        onToast(describe(err));
      });
  };

  /** One field: its label, its value (masked when hidden), Show and Copy. */
  const row = (
    label: string,
    pointer: string,
    mono = false,
  ): JSX.Element | null => {
    const hidden = view.hidden.includes(pointer);
    const path = pointer
      .split("/")
      .slice(1)
      .map((p) => (/^\d+$/.test(p) ? Number(p) : p));
    const value = hidden ? (shown[pointer] ?? MASK) : textAt(item, path);
    if (!hidden && value === "") return null;
    return (
      <div class="detail-row" key={pointer}>
        <span class="label">{label}</span>
        <span class={mono || hidden ? "value mono" : "value"}>{value}</span>
        <span class="row">
          {hidden && (
            <button
              class="link"
              onClick={() => {
                toggle(pointer);
              }}
            >
              {pointer in shown ? "Hide" : "Show"}
            </button>
          )}
          <button
            class="link"
            onClick={() => {
              copy(pointer);
            }}
          >
            Copy
          </button>
        </span>
      </div>
    );
  };

  const urls = getIn(item, ["login", "urls"]);
  const folder = folders.find((f) => f.itemId === getIn(item, ["folder_id"]));
  const tags = getIn(item, ["tags"]);
  const expiry = [
    getIn(item, ["card", "expiry_month"]),
    getIn(item, ["card", "expiry_year"]),
  ]
    .filter((v) => typeof v === "number")
    .map(String)
    .join(" / ");

  return (
    <div class="stack item-detail">
      <div class="row">
        <h2 class="grow">{textAt(item, ["title"]) || "Untitled"}</h2>
        {getIn(item, ["favorite"]) === true && (
          <span class="caption muted">Favorite</span>
        )}
      </div>
      {view.readOnly && (
        <Notice kind="info">Update the app to edit this item.</Notice>
      )}

      {type === "login" && (
        <>
          {row("Username", "/login/username")}
          {row("Password", "/login/password", true)}
          {Array.isArray(urls) &&
            urls.map((_, i) => {
              const url = textAt(item, ["login", "urls", i, "url"]);
              return (
                <div class="detail-row" key={`url-${String(i)}`}>
                  <span class="label">Website</span>
                  <span class="value">
                    {/^https?:\/\//.test(url) ? (
                      <a href={url} target="_blank" rel="noreferrer noopener">
                        {url}
                      </a>
                    ) : (
                      url
                    )}
                  </span>
                  <span class="row">
                    <button
                      class="link"
                      onClick={() => {
                        copy(`/login/urls/${String(i)}/url`);
                      }}
                    >
                      Copy
                    </button>
                  </span>
                </div>
              );
            })}
          {view.code && (
            <div class="detail-row">
              <span class="label">One-time code</span>
              <span class="value mono">
                {view.code.code}{" "}
                <span class="caption muted">{view.code.secondsLeft}s</span>
              </span>
              <span class="row">
                <button
                  class="link"
                  onClick={() => {
                    send({ kind: "copy", vaultId, itemId, field: "code" })
                      .then(() => {
                        onToast("Copied. The clipboard clears in 30 seconds.");
                      })
                      .catch((err: unknown) => {
                        onToast(describe(err));
                      });
                  }}
                >
                  Copy
                </button>
              </span>
            </div>
          )}
          {view.passkeys.map((p) => (
            <div class="detail-row" key={`pk-${p.rpId}-${p.userName}`}>
              <span class="label">Passkey</span>
              <span class="value">
                Passkey for {p.rpId}
                {p.userName && ` (${p.userName})`}
              </span>
              <span />
            </div>
          ))}
        </>
      )}

      {type === "card" && (
        <>
          {row("Cardholder", "/card/cardholder")}
          {row("Number", "/card/number", true)}
          {expiry && (
            <div class="detail-row">
              <span class="label">Expires</span>
              <span class="value">{expiry}</span>
              <span />
            </div>
          )}
          {row("Security code", "/card/security_code", true)}
          {row("Brand", "/card/brand")}
        </>
      )}

      {type === "identity" && (
        <>
          {row("First name", "/identity/first_name")}
          {row("Middle name", "/identity/middle_name")}
          {row("Last name", "/identity/last_name")}
          {row("Email", "/identity/email")}
          {row("Phone", "/identity/phone")}
          {row("Company", "/identity/company")}
          {(Array.isArray(getIn(item, ["identity", "address_lines"]))
            ? (getIn(item, ["identity", "address_lines"]) as unknown[])
            : []
          ).map((_, i) =>
            row("Address", `/identity/address_lines/${String(i)}`),
          )}
          {row("City", "/identity/city")}
          {row("Region", "/identity/region")}
          {row("Postal code", "/identity/postal_code")}
          {row("Country", "/identity/country")}
        </>
      )}

      {customFields(item).map((field, i) =>
        row(
          field.name ||
            FIELD_KINDS.find((k) => k.kind === field.kind)?.label ||
            "Field",
          `/custom_fields/${String(i)}/value`,
        ),
      )}

      {textAt(item, ["notes"]) !== "" && (
        <div class="stack">
          <span class="label">Notes</span>
          <p class="notes">{textAt(item, ["notes"])}</p>
        </div>
      )}
      {folder && <p class="caption muted">Folder: {folder.title}</p>}
      {Array.isArray(tags) && tags.length > 0 && (
        <p class="caption muted">Tags: {tags.join(", ")}</p>
      )}

      {view.passwordHistory.length > 0 && (
        <div class="stack">
          <span class="label">Password history</span>
          {view.passwordHistory.map((past) => (
            <div class="detail-row" key={past.path}>
              <span class="caption muted">
                {new Date(past.savedAt).toLocaleString()}
              </span>
              <span class="value mono">{shown[past.path] ?? MASK}</span>
              <span class="row">
                <button
                  class="link"
                  onClick={() => {
                    toggle(past.path);
                  }}
                >
                  {past.path in shown ? "Hide" : "Show"}
                </button>
              </span>
            </div>
          ))}
        </div>
      )}

      <div class="row">
        {!trashed && !view.readOnly && (
          <button class="primary" onClick={onEdit}>
            Edit
          </button>
        )}
        {!trashed && (
          <button
            onClick={() => {
              act("trashItem", "Moved to the trash.");
            }}
          >
            Delete
          </button>
        )}
        {trashed && (
          <button
            class="primary"
            onClick={() => {
              act("restoreItem", "Restored.");
            }}
          >
            Restore
          </button>
        )}
        {trashed && !confirmDelete && (
          <button
            class="danger"
            onClick={() => {
              setConfirmDelete(true);
            }}
          >
            Delete forever
          </button>
        )}
      </div>
      {confirmDelete && (
        <div class="notice warning stack" role="alert">
          <span>Delete this item forever? It can't be brought back.</span>
          <div class="row">
            <button
              class="danger"
              onClick={() => {
                act("deleteItem", "Deleted.");
              }}
            >
              Delete forever
            </button>
            <button
              onClick={() => {
                setConfirmDelete(false);
              }}
            >
              Keep it
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

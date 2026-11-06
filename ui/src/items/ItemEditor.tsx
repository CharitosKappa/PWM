/**
 * A new item, or an edit (D43 §4, D67 §4): the whole item comes from
 * `itemForEdit` and is changed in place, so fields from newer versions are
 * kept. The core checks the save and keeps the old version in the history.
 * Leaving with changes asks once; nothing else interrupts.
 */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import { describe, send } from "../api.ts";
import type { FolderView, ItemJson } from "../protocol.ts";
import { Notice } from "../ui.tsx";
import {
  FIELD_KINDS,
  addCustomField,
  customFields,
  getIn,
  newItem,
  parseTags,
  removeCustomField,
  setIn,
  textAt,
} from "./model.ts";
import type { FieldKind, ItemType } from "./model.ts";

interface Props {
  /** The item to edit; null for a new one of `type`. */
  editing: { vaultId: string; itemId: string } | null;
  type: ItemType;
  folders: FolderView[];
  /** A folder was made from the picker: the list should read them again. */
  onFoldersChanged: () => void;
  onSaved: (vaultId: string, itemId: string) => void;
  onCancel: () => void;
}

const NEW_FOLDER = "__new__";

function TextInput({
  label,
  value,
  onInput,
  type = "text",
  mono = false,
}: {
  label: string;
  value: string;
  onInput: (value: string) => void;
  type?: "text" | "password" | "email" | "url" | "tel";
  mono?: boolean;
}): JSX.Element {
  return (
    <label class="field">
      <span class="label">{label}</span>
      <input
        class={mono ? "mono" : undefined}
        type={type}
        value={value}
        autoComplete="off"
        spellcheck={false}
        onInput={(e) => {
          onInput(e.currentTarget.value);
        }}
      />
    </label>
  );
}

export function ItemEditor({
  editing,
  type,
  folders,
  onFoldersChanged,
  onSaved,
  onCancel,
}: Props): JSX.Element {
  const [item, setItem] = useState<ItemJson | null>(
    editing ? null : newItem(type),
  );
  const [original, setOriginal] = useState<string>(
    editing ? "" : JSON.stringify(newItem(type)),
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showPassword, setShowPassword] = useState(false);
  const [askDiscard, setAskDiscard] = useState(false);
  const [folderName, setFolderName] = useState<string | null>(null);
  const [tags, setTags] = useState("");

  useEffect(() => {
    if (!editing) return;
    send({ kind: "itemForEdit", ...editing })
      .then((loaded) => {
        setItem(loaded);
        setOriginal(JSON.stringify(loaded));
        const list = loaded.tags;
        setTags(Array.isArray(list) ? list.join(", ") : "");
      })
      .catch((err: unknown) => {
        setError(describe(err));
      });
  }, [editing?.vaultId, editing?.itemId]);

  if (item === null) {
    return error ? (
      <Notice kind="danger">{error}</Notice>
    ) : (
      <div class="stack" aria-busy="true" />
    );
  }

  const itemType = textAt(item, ["type"]);
  // Each change builds on the latest item, so changes in one tick add up.
  const set = (path: (string | number)[], value: unknown): void => {
    setItem((latest) =>
      latest === null ? latest : setIn(latest, path, value),
    );
  };
  const text = (
    label: string,
    path: (string | number)[],
    options: { type?: "text" | "email" | "url" | "tel"; mono?: boolean } = {},
  ): JSX.Element => (
    <TextInput
      label={label}
      value={textAt(item, path)}
      onInput={(v) => {
        set(path, v);
      }}
      {...options}
    />
  );
  const number = (label: string, path: string[]): JSX.Element => {
    const value = getIn(item, path);
    return (
      <label class="field">
        <span class="label">{label}</span>
        <input
          type="number"
          value={typeof value === "number" ? String(value) : ""}
          onInput={(e) => {
            const raw = e.currentTarget.value.trim();
            set(path, raw === "" ? null : Number(raw));
          }}
        />
      </label>
    );
  };

  const dirty = (): boolean =>
    JSON.stringify(setIn(item, ["tags"], parseTags(tags))) !== original;

  const cancel = (): void => {
    if (dirty()) setAskDiscard(true);
    else onCancel();
  };

  const generate = (): void => {
    send({ kind: "generate", mode: "password" })
      .then((value) => {
        set(["login", "password"], value);
        setShowPassword(true);
      })
      .catch((err: unknown) => {
        setError(describe(err));
      });
  };

  const makeFolder = (): void => {
    if (folderName === null) return;
    send({ kind: "newFolder", title: folderName })
      .then(({ itemId }) => {
        set(["folder_id"], itemId);
        setFolderName(null);
        onFoldersChanged();
      })
      .catch((err: unknown) => {
        setError(describe(err));
      });
  };

  const save = async (event: Event): Promise<void> => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      let out = setIn(item, ["tags"], parseTags(tags));
      // A one-time code setup is checked by the core first (D23).
      const totp = textAt(out, ["login", "totp"]).trim();
      if (itemType === "login") {
        out = setIn(
          out,
          ["login", "totp"],
          totp === ""
            ? null
            : await send({
                kind: "totpSetup",
                input: totp,
                title: textAt(out, ["title"]),
              }),
        );
      }
      const saved = await send({
        kind: "saveItem",
        vaultId: editing?.vaultId ?? null,
        item: out,
      });
      onSaved(saved.vaultId, saved.itemId);
    } catch (err) {
      setError(describe(err));
    } finally {
      setBusy(false);
    }
  };

  const urls = getIn(item, ["login", "urls"]);
  const urlList: unknown[] = Array.isArray(urls) ? (urls as unknown[]) : [];
  const lines = getIn(item, ["identity", "address_lines"]);

  return (
    <form class="stack item-editor" onSubmit={(e) => void save(e)}>
      <h2>{editing ? "Edit" : "New"}</h2>
      {text("Title", ["title"])}

      {itemType === "login" && (
        <>
          {text("Username", ["login", "username"])}
          <div class="field">
            <span class="label">Password</span>
            <div class="row">
              <input
                class="mono grow"
                type={showPassword ? "text" : "password"}
                value={textAt(item, ["login", "password"])}
                autoComplete="new-password"
                spellcheck={false}
                aria-label="Password"
                onInput={(e) => {
                  set(["login", "password"], e.currentTarget.value);
                }}
              />
              <button
                type="button"
                class="link"
                onClick={() => {
                  setShowPassword(!showPassword);
                }}
              >
                {showPassword ? "Hide" : "Show"}
              </button>
              <button type="button" onClick={generate}>
                Generate
              </button>
            </div>
          </div>
          {urlList.map((_, i) => (
            <div class="row" key={`url-${String(i)}`}>
              <span class="grow">
                {text("Website", ["login", "urls", i, "url"], { type: "url" })}
              </span>
              <button
                type="button"
                class="link"
                onClick={() => {
                  set(
                    ["login", "urls"],
                    urlList.filter((__, j) => j !== i),
                  );
                }}
              >
                Remove
              </button>
            </div>
          ))}
          <button
            type="button"
            class="link"
            onClick={() => {
              set(
                ["login", "urls"],
                [...urlList, { url: "", match: "domain" }],
              );
            }}
          >
            Add a website
          </button>
          {text("One-time code setup", ["login", "totp"], { mono: true })}
          <span class="caption muted">
            Paste the otpauth:// link or the secret the site shows.
          </span>
        </>
      )}

      {itemType === "card" && (
        <>
          {text("Cardholder", ["card", "cardholder"])}
          {text("Number", ["card", "number"], { mono: true })}
          <div class="row">
            {number("Expiry month", ["card", "expiry_month"])}
            {number("Expiry year", ["card", "expiry_year"])}
          </div>
          {text("Security code", ["card", "security_code"], { mono: true })}
          {text("Brand", ["card", "brand"])}
        </>
      )}

      {itemType === "identity" && (
        <>
          {text("First name", ["identity", "first_name"])}
          {text("Middle name", ["identity", "middle_name"])}
          {text("Last name", ["identity", "last_name"])}
          {text("Email", ["identity", "email"], { type: "email" })}
          {text("Phone", ["identity", "phone"], { type: "tel" })}
          {text("Company", ["identity", "company"])}
          <label class="field">
            <span class="label">Address</span>
            <textarea
              rows={3}
              value={Array.isArray(lines) ? lines.join("\n") : ""}
              onInput={(e) => {
                set(
                  ["identity", "address_lines"],
                  e.currentTarget.value.split("\n"),
                );
              }}
            />
          </label>
          {text("City", ["identity", "city"])}
          {text("Region", ["identity", "region"])}
          {text("Postal code", ["identity", "postal_code"])}
          {text("Country", ["identity", "country"])}
        </>
      )}

      <label class="field">
        <span class="label">Notes</span>
        <textarea
          rows={4}
          value={textAt(item, ["notes"])}
          onInput={(e) => {
            set(["notes"], e.currentTarget.value);
          }}
        />
      </label>

      {customFields(item).map((field, i) => (
        <div class="custom-field" key={`field-${String(i)}`}>
          <TextInput
            label="Field name"
            value={field.name}
            onInput={(v) => {
              set(["custom_fields", i, "name"], v);
            }}
          />
          <TextInput
            label="Value"
            value={field.value}
            type={field.kind === "hidden" ? "password" : "text"}
            mono={field.kind === "hidden"}
            onInput={(v) => {
              set(["custom_fields", i, "value"], v);
            }}
          />
          <label class="field">
            <span class="label">Kind</span>
            <select
              value={field.kind}
              onChange={(e) => {
                set(["custom_fields", i, "kind"], e.currentTarget.value);
              }}
            >
              {FIELD_KINDS.map((k) => (
                <option key={k.kind} value={k.kind}>
                  {k.label}
                </option>
              ))}
            </select>
          </label>
          <button
            type="button"
            class="link"
            onClick={() => {
              setItem((latest) =>
                latest === null ? latest : removeCustomField(latest, i),
              );
            }}
          >
            Remove
          </button>
        </div>
      ))}
      <div class="row">
        {(["text", "hidden"] as FieldKind[]).map((kind) => (
          <button
            type="button"
            class="link"
            key={kind}
            onClick={() => {
              setItem((latest) =>
                latest === null ? latest : addCustomField(latest, kind),
              );
            }}
          >
            {kind === "hidden" ? "Add a hidden field" : "Add a field"}
          </button>
        ))}
      </div>

      <label class="field">
        <span class="label">Folder</span>
        <select
          value={folderName !== null ? NEW_FOLDER : textAt(item, ["folder_id"])}
          onChange={(e) => {
            const value = e.currentTarget.value;
            if (value === NEW_FOLDER) setFolderName("");
            else {
              setFolderName(null);
              set(["folder_id"], value === "" ? null : value);
            }
          }}
        >
          <option value="">None</option>
          {folders.map((f) => (
            <option key={f.itemId} value={f.itemId}>
              {f.title}
            </option>
          ))}
          <option value={NEW_FOLDER}>New folder…</option>
        </select>
      </label>
      {folderName !== null && (
        <div class="row">
          <span class="grow">
            <TextInput
              label="Folder name"
              value={folderName}
              onInput={setFolderName}
            />
          </span>
          <button
            type="button"
            disabled={folderName.trim() === ""}
            onClick={makeFolder}
          >
            Create
          </button>
        </div>
      )}
      <TextInput
        label="Tags, separated by commas"
        value={tags}
        onInput={setTags}
      />
      <label class="row">
        <input
          type="checkbox"
          checked={getIn(item, ["favorite"]) === true}
          onChange={(e) => {
            set(["favorite"], e.currentTarget.checked);
          }}
        />
        <span>Favorite</span>
      </label>

      <div class="slot">{error && <Notice kind="danger">{error}</Notice>}</div>
      {askDiscard ? (
        <div class="notice warning stack" role="alert">
          <span>Discard your changes?</span>
          <div class="row">
            <button type="button" class="danger" onClick={onCancel}>
              Discard
            </button>
            <button
              type="button"
              onClick={() => {
                setAskDiscard(false);
              }}
            >
              Keep editing
            </button>
          </div>
        </div>
      ) : (
        <div class="row">
          <button class="primary" disabled={busy}>
            {busy ? "Saving…" : "Save"}
          </button>
          <button type="button" onClick={cancel}>
            Cancel
          </button>
        </div>
      )}
    </form>
  );
}

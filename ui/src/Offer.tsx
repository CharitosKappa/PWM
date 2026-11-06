/**
 * Save and update offers in the popup (D36 §5, §6): the pending offer for
 * this tab, or "Save login from this page" to read the fields now. The
 * popup never sees the password; the background keeps it.
 */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import type { OfferView } from "./protocol.ts";
import { describe, send } from "./api.ts";
import { Field } from "./ui.tsx";

export function SaveOffer({
  onToast,
}: {
  onToast: (text: string) => void;
}): JSX.Element {
  const [offer, setOffer] = useState<OfferView | null>(null);
  const [title, setTitle] = useState("");
  const [username, setUsername] = useState("");
  const [itemId, setItemId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const show = (view: OfferView | null): void => {
    setOffer(view);
    if (view) {
      setTitle(view.title);
      setUsername(view.username);
      setItemId(view.candidates[0]?.itemId ?? null);
    }
  };

  useEffect(() => {
    send({ kind: "saveOffer" })
      .then(show)
      .catch(() => {
        show(null);
      });
  }, []);

  const run = async (
    work: () => Promise<unknown>,
    done: string,
  ): Promise<void> => {
    setBusy(true);
    try {
      await work();
      show(null);
      if (done !== "") onToast(done);
    } catch (err) {
      onToast(describe(err));
    } finally {
      setBusy(false);
    }
  };

  if (!offer) {
    return (
      <button
        class="link"
        disabled={busy}
        onClick={() => {
          setBusy(true);
          send({ kind: "captureNow" })
            .then(show)
            .catch((err: unknown) => {
              onToast(describe(err));
            })
            .finally(() => {
              setBusy(false);
            });
        }}
      >
        Save login from this page
      </button>
    );
  }

  const update = offer.action === "update";
  return (
    <section class="stack card">
      <h2>
        {update
          ? `Update the password for ${offer.username || offer.site}?`
          : `Save the login for ${offer.site}?`}
      </h2>
      {offer.insecure && (
        <p class="caption warning-text">This page is not secure (http).</p>
      )}
      {!update && (
        <>
          <Field label="Title" value={title} onInput={setTitle} />
          <Field
            label="Username"
            value={username}
            onInput={setUsername}
            autoComplete="off"
          />
        </>
      )}
      {update && offer.candidates.length > 1 && (
        <fieldset class="stack">
          <legend class="label">Which login?</legend>
          {offer.candidates.map((c) => (
            <label key={c.itemId} class="row">
              <input
                type="radio"
                checked={itemId === c.itemId}
                onChange={() => {
                  setItemId(c.itemId);
                }}
              />
              {c.title || "Untitled"}
            </label>
          ))}
        </fieldset>
      )}
      <div class="row">
        <button
          class="primary"
          disabled={busy || offer.locked}
          onClick={() =>
            void run(
              () =>
                send(
                  update
                    ? { kind: "acceptOffer", itemId }
                    : { kind: "acceptOffer", itemId: null, title, username },
                ),
              update ? "Password updated." : "Saved.",
            )
          }
        >
          {update ? "Update" : "Save"}
        </button>
        <button
          disabled={busy}
          onClick={() =>
            void run(() => send({ kind: "dismissOffer", never: false }), "")
          }
        >
          Not now
        </button>
        <button
          class="link"
          disabled={busy}
          onClick={() =>
            void run(
              () => send({ kind: "dismissOffer", never: true }),
              "PWM won't offer to save on this site.",
            )
          }
        >
          Never for this site
        </button>
      </div>
    </section>
  );
}

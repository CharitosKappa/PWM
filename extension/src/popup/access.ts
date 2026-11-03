/**
 * Server access on Firefox and Safari (D40 §1, D48 §4): they treat the
 * manifest's host permissions as needing the user's yes, so the popup asks
 * for the server's origin when the user signs in or creates an account. On
 * Chromium they are granted at install and this asks nothing.
 */
import { useEffect, useState } from "preact/hooks";
import { send } from "./api.ts";

/** Whether this build must ask for server access before signing in. */
export const ASKS_SERVER_ACCESS = PWM_TARGET !== "chromium";

/**
 * The server's origin pattern, loaded ahead of the click: the permission
 * request must be the first thing the click does, so it cannot wait for
 * the settings then.
 */
export function useServerOrigin(): string | null {
  const [origin, setOrigin] = useState<string | null>(null);
  useEffect(() => {
    if (!ASKS_SERVER_ACCESS) return;
    void send({ kind: "settings" }).then((settings) => {
      setOrigin(`${new URL(settings.serverUrl).origin}/*`);
    });
  }, []);
  return origin;
}

/** Asks for the server's origin when this build needs it; true when granted. */
export async function askServerAccess(origin: string | null): Promise<boolean> {
  if (!ASKS_SERVER_ACCESS) return true;
  if (origin === null) return false;
  return chrome.permissions.request({ origins: [origin] });
}

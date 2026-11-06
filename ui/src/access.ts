/**
 * Server access on Firefox and Safari (D40 §1, D48 §4): they treat the
 * manifest's host permissions as needing the user's yes, so the popup asks
 * for the server's origin when the user signs in or creates an account. On
 * Chromium and the desktop app they are granted already and this asks
 * nothing.
 */
import { useEffect, useState } from "preact/hooks";
import { send } from "./api.ts";
import { host } from "./host.ts";

/**
 * The server's origin pattern, loaded ahead of the click: the permission
 * request must be the first thing the click does, so it cannot wait for
 * the settings then.
 */
export function useServerOrigin(): string | null {
  const [origin, setOrigin] = useState<string | null>(null);
  useEffect(() => {
    if (!host().asksServerAccess) return;
    void send({ kind: "settings" }).then((settings) => {
      setOrigin(`${new URL(settings.serverUrl).origin}/*`);
    });
  }, []);
  return origin;
}

/** Asks for the server's origin when this host needs it; true when granted. */
export async function askServerAccess(origin: string | null): Promise<boolean> {
  if (!host().asksServerAccess) return true;
  if (origin === null) return false;
  return host().requestAccess({ origins: [origin] });
}

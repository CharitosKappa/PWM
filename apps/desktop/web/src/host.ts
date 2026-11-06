/**
 * The desktop app as the shared screens' host (D3 §2, D66 §3): requests go
 * to the Rust backend through the app's one command. There is no browser
 * here: no pages to fill and no permissions to ask for. The manage page
 * (M7.6) shares the window with the vault.
 */
import { invoke } from "@tauri-apps/api/core";
import type { Host } from "@pwm/ui/host.ts";
import type { Reply } from "@pwm/ui/protocol.ts";

export const desktopHost: Host = {
  send: (request) => invoke<Reply<unknown>>("request", { request }),
  requestAccess: () => Promise.resolve(true),
  releaseAccess: () => Promise.resolve(),
  openManage: (section) => {
    location.assign(
      section === undefined ? "manage.html" : `manage.html#${section}`,
    );
  },
  closeManage: () => {
    location.assign("index.html");
  },
  browser: false,
  inlineUi: false,
  asksServerAccess: false,
  // Linux unlocks with the master password until polkit or fprintd (D68 §7).
  biometricName: navigator.userAgent.includes("Windows")
    ? "Windows Hello"
    : null,
  manage: true,
  vaultWindow: true,
};

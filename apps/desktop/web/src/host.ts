/**
 * The desktop app as the shared screens' host (D3 §2, D66 §3): requests go
 * to the Rust backend through the app's one command. There is no browser
 * here: no pages to fill, no permissions to ask for, and the manage page
 * comes in M7.6.
 */
import { invoke } from "@tauri-apps/api/core";
import type { Host } from "@pwm/ui/host.ts";
import type { Reply } from "@pwm/ui/protocol.ts";

export const desktopHost: Host = {
  send: (request) => invoke<Reply<unknown>>("request", { request }),
  requestAccess: () => Promise.resolve(true),
  releaseAccess: () => Promise.resolve(),
  openManage: () => undefined,
  browser: false,
  inlineUi: false,
  asksServerAccess: false,
  touchId: false,
  manage: false,
};

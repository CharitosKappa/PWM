/**
 * The extension as the shared screens' host (D3 §2): requests go to the
 * background as popup messages (D34), access through `chrome.permissions`.
 */
import type { Host } from "@pwm/ui/host.ts";
import type { Reply } from "@pwm/ui/protocol.ts";
import type { PopupMessage } from "../shared/messages.ts";

export const chromeHost: Host = {
  send: (request) =>
    chrome.runtime.sendMessage<PopupMessage, Reply<unknown> | undefined>({
      target: "background",
      request,
    }),
  // No await before the request: it must stay the first thing the click does.
  requestAccess: (access) => chrome.permissions.request(access),
  releaseAccess: async (access) => {
    await chrome.permissions.remove(access);
  },
  openManage: (section) => {
    void chrome.tabs.create({
      url: chrome.runtime.getURL(
        section === undefined ? "manage.html" : `manage.html#${section}`,
      ),
    });
  },
  // Only Chromium has the inline icon and the save card (D40 §2). Firefox
  // and Safari ask for server access when signing in (D40 §1, D48 §4).
  inlineUi: PWM_TARGET === "chromium",
  asksServerAccess: PWM_TARGET !== "chromium",
};

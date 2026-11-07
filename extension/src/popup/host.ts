/**
 * The extension as the shared screens' host (D3 §2): requests go to the
 * background as popup messages (D34), access through `chrome.permissions`.
 */
import type { Host } from "@pwm/ui/host.ts";
import type { Reply } from "@pwm/ui/protocol.ts";
import type { PopupMessage } from "../shared/messages.ts";

/**
 * What unlocks the extension through the PWM app on this computer: Touch ID
 * on a Mac (D48 §5), Windows Hello on Windows (D69), Linux login on Linux
 * (D71), nothing on Android, ChromeOS or anything else.
 */
export function biometricName(userAgent: string): string | null {
  if (userAgent.includes("Mac OS X")) return "Touch ID";
  if (userAgent.includes("Windows")) return "Windows Hello";
  if (userAgent.includes("Linux") && !userAgent.includes("Android")) {
    return "Linux login";
  }
  return null;
}

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
  // The manage page is a tab of its own.
  closeManage: null,
  browser: true,
  inlineUi: PWM_TARGET === "chromium",
  // Through the PWM app for Mac (D48 §5), Windows (D69) or Linux (D71).
  biometricName: biometricName(navigator.userAgent),
  manage: true,
  // The popup keeps its list; editing is on the manage page (M4.14).
  vaultWindow: false,
  asksServerAccess: PWM_TARGET !== "chromium",
};

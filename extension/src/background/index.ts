/**
 * The service worker entry (D34). Listeners are registered synchronously,
 * as MV3 requires; they wait for the core to load.
 */
import init, * as wasm from "pwm-core";
import { isPopupMessage } from "../shared/messages.ts";
import type { Reply } from "../shared/messages.ts";
import { chromePlatform } from "./chrome.ts";
import { coreFunctions } from "./functions.ts";
import type { AlarmName } from "./platform.ts";
import { Background, ServiceError } from "./service.ts";

const ALARMS: ReadonlySet<string> = new Set<AlarmName>([
  "autolock",
  "clipboard",
  "rules",
  "sync-retry",
]);

const ready: Promise<Background> = init({
  module_or_path: chrome.runtime.getURL("pwm_wasm_bg.wasm"),
}).then(() => Background.start(coreFunctions(wasm), chromePlatform()));

/** Only the extension's own popup (or options) page may talk to the background. */
function fromPopup(sender: chrome.runtime.MessageSender): boolean {
  if (sender.id !== chrome.runtime.id || sender.url === undefined) return false;
  // The options page is the popup page in a tab, at #settings.
  return sender.url.split("#")[0] === chrome.runtime.getURL("popup.html");
}

chrome.runtime.onMessage.addListener(
  (message: unknown, sender, sendResponse: (reply: Reply<unknown>) => void) => {
    if (!isPopupMessage(message) || !fromPopup(sender)) return false;
    ready
      .then((background) => background.handle(message.request))
      .then(
        (value) => {
          sendResponse({ ok: true, value });
        },
        (error: unknown) => {
          sendResponse({
            ok: false,
            error: error instanceof ServiceError ? error.code : "internal",
          });
        },
      );
    return true;
  },
);

chrome.alarms.onAlarm.addListener((alarm) => {
  if (!ALARMS.has(alarm.name)) return;
  void ready.then((background) => background.onAlarm(alarm.name as AlarmName));
});

chrome.idle.onStateChanged.addListener((state) => {
  if (state === "locked")
    void ready.then((background) => background.onScreenLocked());
});

/** Fetch the rules at startup unless a download is already scheduled (D34). */
async function scheduleRules(): Promise<void> {
  const background = await ready;
  if ((await chrome.alarms.get("rules")) === undefined)
    await background.updateRules();
}

chrome.runtime.onInstalled.addListener(() => {
  void scheduleRules();
});
chrome.runtime.onStartup.addListener(() => {
  void scheduleRules();
});

/**
 * The service worker entry (D34, D35). Listeners are registered
 * synchronously, as MV3 requires; they wait for the core to load. Three
 * kinds of sender: the popup (trusted), content scripts and inline icon
 * frames (untrusted, checked by what the browser says about them).
 */
import init, * as wasm from "pwm-core";
import {
  isFormGone,
  isFrameReport,
  isInlineRequest,
  isSubmitted,
} from "../shared/autofill.ts";
import { isPopupMessage } from "../shared/messages.ts";
import { isPasskeyCall, isPasskeyWindowRequest } from "../shared/passkeys.ts";
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

// The browser tests' build (`build.mjs --test`) reaches the background
// here; the release build compiles this out (and its check makes sure).
if (PWM_TEST) Object.assign(globalThis, { pwmTest: { ready } });

/** Only the extension's own popup (or options) page may talk to the background. */
function fromPopup(sender: chrome.runtime.MessageSender): boolean {
  if (sender.id !== chrome.runtime.id || sender.url === undefined) return false;
  // The options page is the popup page in a tab, at #settings.
  return sender.url.split("#")[0] === chrome.runtime.getURL("popup.html");
}

/** The passkey confirmation window (D37 §4), an extension page in its own window. */
function fromPasskeyWindow(sender: chrome.runtime.MessageSender): boolean {
  if (sender.id !== chrome.runtime.id || sender.url === undefined) return false;
  return sender.url.split("#")[0] === chrome.runtime.getURL("passkey.html");
}

/** The inline icon page, inside a tab (its URL is dynamic, D35 §3). */
function inlineTab(sender: chrome.runtime.MessageSender): number | undefined {
  if (sender.id !== chrome.runtime.id || sender.url === undefined) return;
  const url = new URL(sender.url);
  if (url.protocol !== "chrome-extension:" || url.pathname !== "/inline.html")
    return;
  return sender.tab?.id;
}

/** A content script in a web page; every field comes from the browser. */
function contentFrame(sender: chrome.runtime.MessageSender) {
  const { tab, frameId, documentId, url } = sender;
  if (sender.id !== chrome.runtime.id || tab?.id === undefined) return;
  if (frameId === undefined || documentId === undefined) return;
  if (url === undefined || tab.url === undefined) return;
  if (!/^https?:/.test(url)) return;
  return { tabId: tab.id, frameId, documentId, url, tabUrl: tab.url };
}

function answer(
  work: (background: Background) => Promise<unknown>,
  sendResponse: (reply: Reply<unknown>) => void,
): true {
  ready.then(work).then(
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
}

chrome.runtime.onMessage.addListener(
  (message: unknown, sender, sendResponse: (reply: Reply<unknown>) => void) => {
    if (isPopupMessage(message) && fromPopup(sender)) {
      return answer((b) => b.handle(message.request), sendResponse);
    }
    if (isFrameReport(message)) {
      const frame = contentFrame(sender);
      if (!frame) return false;
      return answer(
        (b) => b.autofill.onFrameReport(frame, message.fields),
        sendResponse,
      );
    }
    if (isPasskeyCall(message)) {
      const frame = contentFrame(sender);
      if (!frame) return false;
      return answer(
        (b) => b.passkeys.onCall(frame, message.op, message.request),
        sendResponse,
      );
    }
    if (isPasskeyWindowRequest(message) && fromPasskeyWindow(sender)) {
      return answer((b) => b.passkeys.onWindow(message), sendResponse);
    }
    if (isSubmitted(message) || isFormGone(message)) {
      const frame = contentFrame(sender);
      if (!frame) return false;
      return answer(
        (b) =>
          isSubmitted(message)
            ? b.saver.onSubmitted(frame, message.typed)
            : b.saver.onFormGone(frame),
        sendResponse,
      );
    }
    if (isInlineRequest(message)) {
      const tabId = inlineTab(sender);
      if (tabId === undefined) return false;
      return answer(
        (b) =>
          b.saver.owns(message.token)
            ? b.saver.onInline(message, tabId)
            : b.autofill.onInline(message, tabId),
        sendResponse,
      );
    }
    return false;
  },
);

chrome.commands.onCommand.addListener((command, tab) => {
  if (command !== "fill-login") return;
  const target =
    tab?.id !== undefined && tab.url !== undefined
      ? { id: tab.id, url: tab.url }
      : undefined;
  void ready.then((b) => b.autofill.onCommand(target));
});

chrome.windows.onRemoved.addListener((windowId) => {
  void ready.then((b) => b.passkeys.onWindowClosed(windowId));
});

chrome.tabs.onRemoved.addListener((tabId) => {
  void ready.then((b) => {
    b.autofill.onTabRemoved(tabId);
    b.saver.onTabRemoved(tabId);
    b.reporter.onTabRemoved(tabId);
  });
});

// Taking back site access in the browser turns the inline icon off.
chrome.permissions.onRemoved.addListener(() => {
  void ready.then((b) => b.autofill.dropInlineIfNoAccess());
});

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

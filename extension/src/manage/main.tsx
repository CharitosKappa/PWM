import { render } from "preact";
import { setHost } from "@pwm/ui/host.ts";
import { Manage } from "@pwm/ui/Manage.tsx";
import { chromeHost } from "../popup/host.ts";
import { isLockedMessage } from "../shared/messages.ts";

setHost(chromeHost);
// A lock reloads the page, so nothing revealed outlives it (D67 §1).
chrome.runtime.onMessage.addListener((message: unknown) => {
  if (isLockedMessage(message)) location.reload();
});
const root = document.getElementById("app");
if (root) render(<Manage />, root);

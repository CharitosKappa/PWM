import { render } from "preact";
import { App } from "@pwm/ui/App.tsx";
import { setHost } from "@pwm/ui/host.ts";
import { chromeHost } from "./host.ts";

setHost(chromeHost);
const root = document.getElementById("app");
// The same page serves as the options page, opened at #settings.
if (root) render(<App startInSettings={location.hash === "#settings"} />, root);

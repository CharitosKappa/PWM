import { render } from "preact";
import { App } from "@pwm/ui/App.tsx";
import { setHost } from "@pwm/ui/host.ts";
import { desktopHost } from "./host.ts";

setHost(desktopHost);
const root = document.getElementById("app");
if (root) render(<App startInSettings={false} />, root);

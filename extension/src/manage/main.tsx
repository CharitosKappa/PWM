import { render } from "preact";
import { setHost } from "@pwm/ui/host.ts";
import { Manage } from "@pwm/ui/Manage.tsx";
import { chromeHost } from "../popup/host.ts";

setHost(chromeHost);
const root = document.getElementById("app");
if (root) render(<Manage />, root);

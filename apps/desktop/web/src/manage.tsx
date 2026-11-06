import { render } from "preact";
import { setHost } from "@pwm/ui/host.ts";
import { Manage } from "@pwm/ui/Manage.tsx";
import { desktopHost } from "./host.ts";

setHost(desktopHost);
const root = document.getElementById("app");
if (root) render(<Manage />, root);

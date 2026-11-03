import { render } from "preact";
import { Manage } from "./Manage.tsx";

const root = document.getElementById("app");
if (root) render(<Manage />, root);

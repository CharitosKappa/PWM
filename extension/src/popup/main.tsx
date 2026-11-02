import { render } from "preact";
import { App } from "./App.tsx";

const root = document.getElementById("app");
// The same page serves as the options page, opened at #settings.
if (root) render(<App startInSettings={location.hash === "#settings"} />, root);

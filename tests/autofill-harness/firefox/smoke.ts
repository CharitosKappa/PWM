/**
 * Firefox smoke test (D40 §4). Playwright cannot load an extension into
 * Firefox, so this drives geckodriver over plain W3C WebDriver: it installs
 * the Firefox build as a temporary add-on, opens its popup page, and scores
 * a password. The score comes from the WASM core in the background page, so
 * a reply proves the background started and answers messages.
 *
 *   node firefox/smoke.ts ../../extension/dist/pwm-firefox-<version>.zip
 *
 * Needs Firefox and geckodriver (`GECKOWEBDRIVER` or on PATH). Signing up and
 * filling are not covered yet: Firefox asks for host access in a prompt that
 * WebDriver cannot answer.
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

/** The add-on id in manifests/firefox.json. */
const GECKO_ID = "pwm@pwm.invalid";
/** Fixed, so the test knows the popup's moz-extension:// address. */
const UUID = "6f1d3c2a-5b4e-4f70-9a8b-1c2d3e4f5a6b";
const PORT = 4444;
const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";

const zip = process.argv[2];
if (zip === undefined || !fs.existsSync(zip)) {
  console.error("usage: node firefox/smoke.ts <pwm-firefox-*.zip>");
  process.exit(2);
}

const geckodriver =
  process.env.GECKOWEBDRIVER === undefined
    ? "geckodriver"
    : path.join(process.env.GECKOWEBDRIVER, "geckodriver");
const driver = spawn(geckodriver, [`--port=${String(PORT)}`], {
  stdio: "ignore",
});

type Json = Record<string, unknown>;

async function call(
  method: string,
  url: string,
  body?: Json,
): Promise<unknown> {
  const response = await fetch(`http://127.0.0.1:${String(PORT)}${url}`, {
    method,
    headers: { "content-type": "application/json" },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const reply = (await response.json()) as { value: unknown };
  if (!response.ok) {
    throw new Error(`${method} ${url}: ${JSON.stringify(reply.value)}`);
  }
  return reply.value;
}

async function until<T>(
  what: string,
  poll: () => Promise<T | null>,
): Promise<T> {
  const deadline = Date.now() + 20_000;
  for (;;) {
    const value = await poll().catch(() => null);
    if (value !== null) return value;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}

let session: string | null = null;
try {
  await until("geckodriver", () => call("GET", "/status"));
  const created = await call("POST", "/session", {
    capabilities: {
      alwaysMatch: {
        browserName: "firefox",
        "moz:firefoxOptions": {
          args: ["-headless"],
          prefs: {
            "extensions.webextensions.uuids": JSON.stringify({
              [GECKO_ID]: UUID,
            }),
          },
        },
      },
    },
  });
  session = (created as Json).sessionId as string;
  const base = `/session/${session}`;

  await call("POST", `${base}/moz/addon/install`, {
    addon: fs.readFileSync(zip).toString("base64"),
    temporary: true,
  });
  await call("POST", `${base}/url`, {
    url: `moz-extension://${UUID}/popup.html`,
  });

  const find = async (selector: string): Promise<string> => {
    const found = await call("POST", `${base}/element`, {
      using: "css selector",
      value: selector,
    });
    return (found as Json)[ELEMENT] as string;
  };
  const tab = await until("the signed-out view", async () => {
    const id = await find('button[role="tab"]:last-child');
    const text = await call("GET", `${base}/element/${id}/text`);
    return text === "Create account" ? id : null;
  });
  await call("POST", `${base}/element/${tab}/click`, {});
  const password = await until("the sign-up form", () =>
    find('input[autocomplete="new-password"]'),
  );
  await call("POST", `${base}/element/${password}/value`, {
    text: "smoke test passphrase for firefox",
  });
  const score = await until("a score from the background", async () => {
    const meter = await find(".meter");
    const value = await call(
      "GET",
      `${base}/element/${meter}/attribute/data-score`,
    );
    return typeof value === "string" && value !== "" ? value : null;
  });
  console.log(`firefox smoke: popup loaded, background scored ${score}/4`);
} catch (error) {
  console.error(`firefox smoke: ${String(error)}`);
  process.exitCode = 1;
} finally {
  if (session !== null) {
    await call("DELETE", `/session/${session}`).catch(() => null);
  }
  driver.kill();
}

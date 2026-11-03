// Approving a new device (D46) from the extension's manage page, against
// the local sync server. The new device is the core's WASM build in this
// test process: it asks, shows its code, and logs in with the Secret Key
// the extension sends once the codes match.
import { expect, test } from "@playwright/test";
import { createRequire } from "node:module";
import path from "node:path";
import { Harness, SERVER } from "../lib/extension.ts";

const MASTER = "correct horse battery staple umbrella";

interface Step {
  kind: string;
  method?: string;
  path?: string;
  headers: string[];
  body?: Uint8Array;
  outcome?: string;
}
interface AskStep {
  kind: string;
  code?: string;
  secretKey?: string;
  request(): Step | undefined;
}
interface Wasm {
  WasmApprovalAsk: new (
    email: string,
    platform: string,
  ) => {
    firstRequest(): Step | undefined;
    poll(): Step;
    receive(status: number, headers: string[], body: Uint8Array): AskStep;
  };
  WasmClient: new () => {
    startLogin(
      email: string,
      password: string,
      secretKey: string,
      deviceId: string,
      platform: string,
      deviceName: string,
    ): Step;
    receive(
      status: number,
      headers: string[],
      body: Uint8Array,
      now: number,
    ): Step;
  };
  newId(): string;
}

const wasm = createRequire(import.meta.url)(
  process.env.PWM_WASM_NODE ??
    path.resolve(import.meta.dirname, "../../../target/wasm/node/pwm_wasm.js"),
) as Wasm;

/** Sends one step's request; returns what the core reads. */
async function fetchStep(step: Step): Promise<[number, string[], Uint8Array]> {
  const headers = new Headers();
  for (let i = 0; i + 1 < step.headers.length; i += 2) {
    headers.append(step.headers[i] ?? "", step.headers[i + 1] ?? "");
  }
  const response = await fetch(SERVER + (step.path ?? "/"), {
    method: step.method ?? "GET",
    headers,
    body: step.body ? Buffer.from(step.body) : null,
  });
  const pairs: string[] = [];
  response.headers.forEach((value, name) => {
    pairs.push(name, value);
  });
  return [response.status, pairs, new Uint8Array(await response.arrayBuffer())];
}

let harness: Harness;

test.beforeAll(async () => {
  harness = await Harness.start();
});

test.afterAll(async () => {
  await harness.close();
});

test("the extension approves a new device, which then logs in", async () => {
  const email = await harness.inBackground(async () => {
    const b = await globalThis.pwmTest.ready;
    const status = (await b.handle({ kind: "status" })) as { email: string };
    return status.email;
  }, null);

  // The new device asks.
  const ask = new wasm.WasmApprovalAsk(email, "ios");
  const first = ask.firstRequest();
  if (!first) throw new Error("no first request");
  let step = ask.receive(...(await fetchStep(first)));
  expect(step.kind).toBe("wait");

  // The extension sees it on the manage page and approves it.
  const id = new URL(harness.worker.url()).host;
  const page = await harness.context.newPage();
  await page.goto(`chrome-extension://${id}/manage.html#devices`);
  const approvals = page.locator("#approvals");
  await approvals.getByRole("button", { name: "Approve" }).click();
  await expect(approvals).toContainText("Waiting for the new device");

  // The new device reveals its key and shows its code.
  let code: string | undefined;
  for (let i = 0; i < 20 && code === undefined; i++) {
    step = ask.receive(...(await fetchStep(ask.poll())));
    if (step.kind === "send") {
      const reveal = step.request();
      if (!reveal) throw new Error("no reveal");
      step = ask.receive(...(await fetchStep(reveal)));
    }
    if (step.kind === "code") code = step.code;
  }
  expect(code).toMatch(/^\d{3} \d{3}$/);
  await expect(page.locator("#approval-code")).toHaveText(code ?? "", {
    timeout: 10_000,
  });

  // The codes match: the extension sends the Secret Key.
  await page.locator("#codes-match").click();
  await expect(approvals).toContainText("Approved");
  step = ask.receive(...(await fetchStep(ask.poll())));
  expect(step.kind).toBe("approved");
  const secretKey = step.secretKey ?? "";
  expect(secretKey).toMatch(/^A\d-/);

  // With it and the master password, the new device logs in.
  const client = new wasm.WasmClient();
  let login = client.startLogin(
    email,
    MASTER,
    secretKey,
    wasm.newId(),
    "ios",
    "Test iPhone",
  );
  while (login.kind === "send") {
    const [status, headers, body] = await fetchStep(login);
    login = client.receive(status, headers, body, Date.now());
  }
  expect(login.outcome).toBe("logged_in");
});

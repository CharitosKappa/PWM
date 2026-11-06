import { beforeEach, expect, test, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { desktopHost } = await import("./host.ts");

beforeEach(() => {
  invoke.mockReset();
});

test("a request goes to the backend's one command", async () => {
  invoke.mockResolvedValue({ ok: true, value: { state: "signedOut" } });
  await expect(desktopHost.send({ kind: "status" })).resolves.toEqual({
    ok: true,
    value: { state: "signedOut" },
  });
  expect(invoke).toHaveBeenCalledWith("request", {
    request: { kind: "status" },
  });
});

test("nothing browser-only shows, the manage page does, and nothing asks for access", async () => {
  expect(desktopHost.browser).toBe(false);
  expect(desktopHost.inlineUi).toBe(false);
  expect(desktopHost.touchId).toBe(false);
  expect(desktopHost.manage).toBe(true);
  expect(desktopHost.closeManage).not.toBeNull();
  expect(desktopHost.asksServerAccess).toBe(false);
  await expect(
    desktopHost.requestAccess({ origins: ["https://*/*"] }),
  ).resolves.toBe(true);
  expect(invoke).not.toHaveBeenCalled();
});

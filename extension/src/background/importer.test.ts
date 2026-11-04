/**
 * Import from the manage page (D51 §9), over the fake core: detect, preview,
 * save and sync; nothing written to disk; refusals with their codes.
 */
import { describe, expect, it } from "vitest";
import { toBase64 } from "../shared/bytes.ts";
import {
  PASSWORD,
  type FakeCore,
  fakeFunctions,
  fakePlatform,
} from "./fakes.ts";
import { MAX_IMPORT_BYTES } from "./importer.ts";
import { Background, ServiceError } from "./service.ts";

const LASTPASS =
  "url,username,password,totp,extra,name,grouping,fav\nBank,ada,pw\nMail,ada,pw2\n";

const file = (text: string) => toBase64(new TextEncoder().encode(text));

async function signedUp() {
  const platform = fakePlatform();
  const fns = fakeFunctions();
  const background = await Background.start(fns, platform);
  await background.handle({
    kind: "signUp",
    email: "ada@example.com",
    password: PASSWORD,
  });
  const core = fns.cores.at(-1) as FakeCore;
  return { background, platform, core };
}

async function errorOf(promise: Promise<unknown>): Promise<string> {
  try {
    await promise;
  } catch (error) {
    if (error instanceof ServiceError) return error.code;
    throw error;
  }
  throw new Error("no error");
}

describe("import", () => {
  it("detects, previews, then saves and syncs", async () => {
    const { background, platform, core } = await signedUp();
    const detected = await background.handle({
      kind: "importDetect",
      name: "lastpass.csv",
      file: file(LASTPASS),
    });
    expect(detected).toEqual({
      format: "lastpass",
      needsPassword: false,
      columns: null,
    });

    const preview = await background.handle({
      kind: "importPrepare",
      file: file(LASTPASS),
      format: "lastpass",
      password: "",
      columns: null,
    });
    expect(preview.logins).toBe(2);
    expect(preview.leftOut).toEqual([{ reason: "attachments", count: 1 }]);
    const before = await background.handle({ kind: "items", query: "" });

    const done = await background.handle({ kind: "importFinish" });
    expect(done.logins).toBe(2);
    const after = await background.handle({ kind: "items", query: "" });
    expect(after.length).toBe(before.length + 2);
    // Never written to disk as it was.
    const disk = JSON.stringify([...platform.disk.values]);
    expect(disk).not.toContain("url,username,password");
    expect(core.pendingImport).toBeNull();
  });

  it("gives a generic CSV's columns for the picker", async () => {
    const { background } = await signedUp();
    const detected = await background.handle({
      kind: "importDetect",
      name: "passwords.csv",
      file: file("name,url,username,password\nBank,https://b.example,ada,pw\n"),
    });
    expect(detected.format).toBe("csv");
    expect(detected.columns?.headers).toEqual([
      "name",
      "url",
      "username",
      "password",
    ]);
    expect(detected.columns?.suggested.title).toBe(0);
    expect(detected.columns?.suggested.url).toBeNull();
  });

  it("asks for an encrypted export's password", async () => {
    const { background } = await signedUp();
    const json = file('{"encrypted":true}');
    const detected = await background.handle({
      kind: "importDetect",
      name: "bitwarden.json",
      file: json,
    });
    expect(detected).toMatchObject({
      format: "bitwarden",
      needsPassword: true,
    });
    const prepare = (password: string) =>
      background.handle({
        kind: "importPrepare",
        file: json,
        format: "bitwarden",
        password,
        columns: null,
      });
    expect(await errorOf(prepare(""))).toBe("import_password_needed");
    expect(await errorOf(prepare("nope"))).toBe("import_wrong_password");
    expect((await prepare("export-pw")).logins).toBe(0);
  });

  it("refuses files it can't read, too large or while locked", async () => {
    const { background, core } = await signedUp();
    const unknown = await background.handle({
      kind: "importDetect",
      name: "photo.png",
      file: file("\u0089PNG"),
    });
    expect(unknown.format).toBeNull();
    const huge = "A".repeat(Math.ceil(MAX_IMPORT_BYTES / 3) * 4 + 4);
    expect(
      await errorOf(
        background.handle({ kind: "importDetect", name: "x.csv", file: huge }),
      ),
    ).toBe("import_too_large");
    expect(await errorOf(background.handle({ kind: "importFinish" }))).toBe(
      "not_running",
    );

    await background.handle({ kind: "lock" });
    expect(
      await errorOf(
        background.handle({
          kind: "importPrepare",
          file: file(LASTPASS),
          format: "lastpass",
          password: "",
          columns: null,
        }),
      ),
    ).toBe("locked");
    expect(core.importsPrepared).toBe(0);
  });

  it("drops a preview on cancel", async () => {
    const { background, core } = await signedUp();
    await background.handle({
      kind: "importPrepare",
      file: file(LASTPASS),
      format: "lastpass",
      password: "",
      columns: null,
    });
    await background.handle({ kind: "importCancel" });
    expect(core.pendingImport).toBeNull();
  });
});

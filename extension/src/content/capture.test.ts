import { describe, expect, it } from "vitest";
import { readTyped } from "./capture.ts";

/** Inputs as far as readTyped looks at them. */
const inputs = (...values: string[]) =>
  values.map((value) => ({ value }) as HTMLInputElement);

describe("readTyped", () => {
  it("reads only the named fields", () => {
    expect(
      readTyped(inputs("ada", "secret", "not-read"), {
        username: 0,
        password: 1,
        newPassword: [],
      }),
    ).toEqual({ username: "ada", password: "secret", newPassword: "" });
  });

  it("takes the new password only when both fields agree", () => {
    const watch = { username: null, password: 0, newPassword: [1, 2] };
    expect(readTyped(inputs("old", "new", "new"), watch)).toEqual({
      username: "",
      password: "old",
      newPassword: "new",
    });
    expect(readTyped(inputs("old", "new", "typo"), watch).newPassword).toBe("");
    expect(
      readTyped(inputs("only"), {
        username: null,
        password: null,
        newPassword: [0],
      }).newPassword,
    ).toBe("only");
  });

  it("reads nothing from fields that are gone", () => {
    expect(
      readTyped(inputs(), { username: 3, password: 4, newPassword: [5] }),
    ).toEqual({ username: "", password: "", newPassword: "" });
  });
});

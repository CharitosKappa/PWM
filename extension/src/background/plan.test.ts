import { describe, expect, it } from "vitest";
import { field } from "./fakes.ts";
import { iconFields, planFill } from "./plan.ts";
import type { FieldClass } from "./plan.ts";

const ALL = { username: true, password: true, code: true };
const c = (cls: string, confidence = 0.9): FieldClass => ({
  class: cls,
  confidence,
});

describe("planFill", () => {
  it("fills the username just before the password in the same form", () => {
    const fields = [
      field(0, { form: 0 }), // newsletter email in another form below
      field(1, { form: 1 }),
      field(2, { form: 1 }),
      field(3, { form: 1, type: "password" }),
    ];
    const classes = [
      c("email"),
      c("username"),
      c("email"),
      c("current-password"),
    ];
    expect(planFill(fields, classes, ALL)).toEqual({
      username: 2,
      password: 3,
      copyCode: false,
      step: "password",
    });
  });

  it("never fills hidden fields or new-password fields", () => {
    const fields = [
      field(0, { visible: false }),
      field(1, {}),
      field(2, { type: "password", visible: false }),
      field(3, { type: "password" }),
    ];
    const classes = [
      c("username"),
      c("username"),
      c("current-password"),
      c("new-password"),
    ];
    expect(planFill(fields, classes, ALL)).toEqual({
      username: 1,
      copyCode: false,
      step: "username",
    });
  });

  it("fills the username alone on the first page of a multi-step login", () => {
    const plan = planFill([field(0, {})], [c("email")], ALL);
    expect(plan).toEqual({ username: 0, copyCode: false, step: "username" });
  });

  it("fills a code only at 0.8 confidence or more, else copies it", () => {
    expect(planFill([field(0, {})], [c("one-time-code", 0.95)], ALL)).toEqual({
      code: 0,
      copyCode: false,
      step: null,
    });
    expect(planFill([field(0, {})], [c("one-time-code", 0.6)], ALL)).toEqual({
      copyCode: true,
      step: null,
    });
  });

  it("copies the code when the login in progress expects one next", () => {
    expect(planFill([field(0, {})], [c("ignore")], ALL, "totp")).toEqual({
      copyCode: true,
      step: null,
    });
    expect(
      planFill([field(0, {})], [c("ignore")], { ...ALL, code: false }, "totp"),
    ).toBeNull();
  });

  it("finds nothing on a page without login fields", () => {
    expect(
      planFill(
        [field(0, {}), field(1, {})],
        [c("ignore"), c("cc-number")],
        ALL,
      ),
    ).toBeNull();
  });

  it("leaves out what the login does not have", () => {
    const fields = [field(0, {}), field(1, { type: "password" })];
    const classes = [c("username"), c("current-password")];
    expect(
      planFill(fields, classes, {
        username: false,
        password: true,
        code: false,
      }),
    ).toEqual({
      password: 1,
      copyCode: false,
      step: "password",
    });
  });
});

describe("iconFields", () => {
  it("puts icons on the fields a fill would use", () => {
    const fields = [field(0, {}), field(1, { type: "password" }), field(2, {})];
    expect(
      iconFields(fields, [c("username"), c("current-password"), c("ignore")]),
    ).toEqual([0, 1]);
    expect(iconFields(fields, [c("ignore"), c("ignore"), c("ignore")])).toEqual(
      [],
    );
  });
});

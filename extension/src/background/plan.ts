/**
 * Which fields of a frame get which value (D35 §2), from the engine's
 * classification. Pure, so it is tested without a browser.
 */
import type { FieldInfo, Watch } from "../shared/autofill.ts";

export interface FieldClass {
  class: string;
  confidence: number;
}

export interface Plan {
  username?: number;
  password?: number;
  code?: number;
  /** The page wants a code but no field is surely one: copy it instead. */
  copyCode: boolean;
  /** The multi-step state to record after filling. */
  step: "username" | "password" | null;
}

export interface Wants {
  username: boolean;
  password: boolean;
  code: boolean;
}

/** Codes are filled only into fields classified at this confidence or more (D33). */
export const CODE_CONFIDENCE = 0.8;

const USERNAME = new Set(["username", "email"]);

export function planFill(
  fields: FieldInfo[],
  classes: FieldClass[],
  wants: Wants,
  nextStep?: string,
): Plan | null {
  const all = fields.map((field, i) => ({
    field,
    cls: classes[i] ?? { class: "ignore", confidence: 0 },
  }));
  const visible = all.filter((c) => c.field.visible);
  const plan: Plan = { copyCode: false, step: null };

  const password = visible.find((c) => c.cls.class === "current-password");
  if (password && wants.password) plan.password = password.field.index;

  if (wants.username) {
    const users = visible.filter((c) => USERNAME.has(c.cls.class));
    let user = users[0];
    if (password) {
      const sameForm = users.filter(
        (c) => c.field.form === password.field.form,
      );
      // The username field just before the password, else any in its form.
      user =
        sameForm.filter((c) => c.field.index < password.field.index).at(-1) ??
        sameForm[0];
    }
    if (user) plan.username = user.field.index;
  }

  if (wants.code) {
    const codes = visible.filter((c) => c.cls.class === "one-time-code");
    const sure = codes.find((c) => c.cls.confidence >= CODE_CONFIDENCE);
    // A code split over several boxes (one digit each) is copied: the
    // page's own script spreads a paste over them, a fill would not.
    const split =
      codes.length > 1 ||
      (sure?.field.max_length !== undefined &&
        sure.field.max_length !== null &&
        sure.field.max_length < 6);
    if (sure && !split) plan.code = sure.field.index;
    else if (codes.length > 0 || nextStep === "totp") plan.copyCode = true;
  }

  if (plan.password !== undefined) plan.step = "password";
  else if (plan.username !== undefined) plan.step = "username";

  const nothing =
    plan.username === undefined &&
    plan.password === undefined &&
    plan.code === undefined;
  return nothing && !plan.copyCode ? null : plan;
}

/**
 * Which fields to read on submit (D36 §2): the username a fill would use,
 * the current password, and up to two new-password fields (sign-up and
 * change-password forms). Null when the frame has none of them.
 */
export function watchFields(
  fields: FieldInfo[],
  classes: FieldClass[],
): Watch | null {
  const visible = fields
    .map((field, i) => ({ field, cls: classes[i] }))
    .filter((c) => c.field.visible);
  const plan = planFill(fields, classes, {
    username: true,
    password: true,
    code: false,
  });
  const newPassword = visible
    .filter((c) => c.cls?.class === "new-password")
    .slice(0, 2)
    .map((c) => c.field.index);
  const watch: Watch = {
    username: plan?.username ?? null,
    password: plan?.password ?? null,
    newPassword,
  };
  if (
    watch.username === null &&
    watch.password === null &&
    newPassword.length === 0
  )
    return null;
  return watch;
}

/** Where the inline icon goes: the login fields a fill would use. */
export function iconFields(
  fields: FieldInfo[],
  classes: FieldClass[],
): number[] {
  const plan = planFill(fields, classes, {
    username: true,
    password: true,
    code: true,
  });
  if (!plan) return [];
  return [plan.username, plan.password, plan.code].filter(
    (i): i is number => i !== undefined,
  );
}

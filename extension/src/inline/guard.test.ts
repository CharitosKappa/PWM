import { describe, expect, it } from "vitest";
import { ClickGuard, MIN_VISIBLE_MS } from "./guard.ts";

function guard() {
  const clock = { now: 0 };
  return { clock, guard: new ClickGuard(() => clock.now) };
}

const trusted = { isTrusted: true };

describe("ClickGuard", () => {
  it("takes a trusted click after 500 ms of being visible", () => {
    const { clock, guard: g } = guard();
    g.observed(true);
    clock.now = MIN_VISIBLE_MS - 1;
    expect(g.accepts(trusted)).toBe(false);
    clock.now = MIN_VISIBLE_MS;
    expect(g.accepts(trusted)).toBe(true);
  });

  it("refuses synthetic clicks", () => {
    const { clock, guard: g } = guard();
    g.observed(true);
    clock.now = 10_000;
    expect(g.accepts({ isTrusted: false })).toBe(false);
  });

  it("refuses clicks while covered or transparent, and restarts the wait after", () => {
    const { clock, guard: g } = guard();
    g.observed(true);
    clock.now = 1000;
    g.observed(false);
    expect(g.accepts(trusted)).toBe(false);
    g.observed(true);
    clock.now = 1000 + MIN_VISIBLE_MS - 1;
    expect(g.accepts(trusted)).toBe(false);
    clock.now = 1000 + MIN_VISIBLE_MS;
    expect(g.accepts(trusted)).toBe(true);
  });

  it("refuses everything where IntersectionObserver v2 is missing", () => {
    const { clock, guard: g } = guard();
    g.observed(undefined);
    clock.now = 10_000;
    expect(g.accepts(trusted)).toBe(false);
  });

  it("waits again after the view changes (icon to menu)", () => {
    const { clock, guard: g } = guard();
    g.observed(true);
    clock.now = 5000;
    g.viewChanged();
    clock.now = 5000 + MIN_VISIBLE_MS - 1;
    expect(g.accepts(trusted)).toBe(false);
    clock.now = 5000 + MIN_VISIBLE_MS;
    expect(g.accepts(trusted)).toBe(true);
  });
});

import { describe, expect, it } from "vitest";
import { fieldProblem, inViewport, samplePoints } from "./checks.ts";
import type { FieldState, StyleInfo } from "./checks.ts";

const plain: StyleInfo = {
  display: "block",
  visibility: "visible",
  opacity: "1",
  filter: "none",
  clipPath: "none",
};

function state(patch: Partial<FieldState> = {}): FieldState {
  return {
    connected: true,
    disabled: false,
    readOnly: false,
    rect: { left: 10, top: 10, width: 200, height: 30 },
    viewport: { width: 1000, height: 800 },
    styles: [plain, plain, plain],
    onTop: [true, true, true],
    visibleV2: true,
    topFrame: true,
    ...patch,
  };
}

describe("fieldProblem", () => {
  it("passes a plain visible field", () => {
    expect(fieldProblem(state())).toBeNull();
  });

  it.each([
    ["gone", { connected: false }],
    ["disabled", { disabled: true }],
    ["read_only", { readOnly: true }],
    ["no_size", { rect: { left: 10, top: 10, width: 0, height: 30 } }],
    ["off_screen", { rect: { left: 10, top: 790, width: 200, height: 30 } }],
    ["off_screen", { rect: { left: -5, top: 10, width: 200, height: 30 } }],
  ] as [string, Partial<FieldState>][])("refuses: %s", (problem, patch) => {
    expect(fieldProblem(state(patch))).toBe(problem);
  });

  it("refuses a field with a transparent or hidden ancestor", () => {
    expect(
      fieldProblem(state({ styles: [plain, { ...plain, opacity: "0" }] })),
    ).toBe("transparent");
    expect(
      fieldProblem(state({ styles: [plain, { ...plain, opacity: "0.99" }] })),
    ).toBe("transparent");
    expect(
      fieldProblem(state({ styles: [{ ...plain, visibility: "hidden" }] })),
    ).toBe("hidden");
    expect(
      fieldProblem(state({ styles: [plain, { ...plain, display: "none" }] })),
    ).toBe("hidden");
    expect(
      fieldProblem(
        state({ styles: [plain, { ...plain, filter: "opacity(0)" }] }),
      ),
    ).toBe("transparent");
    expect(
      fieldProblem(state({ styles: [{ ...plain, clipPath: "inset(50%)" }] })),
    ).toBe("hidden");
  });

  it("refuses a field covered at any sample point", () => {
    expect(fieldProblem(state({ onTop: [true, false, true] }))).toBe("covered");
    expect(fieldProblem(state({ onTop: [] }))).toBe("covered");
  });

  it("refuses what IntersectionObserver v2 reports as not visible", () => {
    expect(fieldProblem(state({ visibleV2: false }))).toBe("not_visible");
  });

  it("without v2, fills only in the top frame", () => {
    expect(fieldProblem(state({ visibleV2: undefined }))).toBeNull();
    expect(fieldProblem(state({ visibleV2: undefined, topFrame: false }))).toBe(
      "frame_unchecked",
    );
  });
});

describe("geometry", () => {
  it("samples the centre and two inner corners", () => {
    expect(samplePoints({ left: 0, top: 0, width: 100, height: 20 })).toEqual([
      [50, 10],
      [2, 2],
      [98, 18],
    ]);
  });

  it("keeps sample points inside tiny fields", () => {
    for (const [x, y] of samplePoints({
      left: 0,
      top: 0,
      width: 4,
      height: 4,
    })) {
      expect(x).toBeGreaterThanOrEqual(0);
      expect(x).toBeLessThanOrEqual(4);
      expect(y).toBeGreaterThanOrEqual(0);
      expect(y).toBeLessThanOrEqual(4);
    }
  });

  it("needs the whole field in the viewport", () => {
    expect(
      inViewport(
        { left: 0, top: 0, width: 10, height: 10 },
        { width: 10, height: 10 },
      ),
    ).toBe(true);
    expect(
      inViewport(
        { left: 1, top: 0, width: 10, height: 10 },
        { width: 10, height: 10 },
      ),
    ).toBe(false);
  });
});

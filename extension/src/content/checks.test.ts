import { describe, expect, it } from "vitest";
import {
  fieldProblem,
  insideField,
  inViewport,
  samplePoints,
  unreachable,
} from "./checks.ts";
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
    onTop: ["field", "field", "field"],
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
    expect(fieldProblem(state({ onTop: ["field", "other", "field"] }))).toBe(
      "covered",
    );
    expect(fieldProblem(state({ onTop: [] }))).toBe("covered");
  });

  it("counts the field's own ancestor on top as clear (D63 §2)", () => {
    // Outside a rounded corner the field doesn't paint; its wrapper is hit.
    expect(
      fieldProblem(state({ onTop: ["field", "ancestor", "ancestor"] })),
    ).toBeNull();
    expect(
      fieldProblem(state({ onTop: ["ancestor", "other", "ancestor"] })),
    ).toBe("covered");
  });

  it("lets a button inside the field sit at a corner (D65 §3)", () => {
    // A show-password button at the field's right end.
    expect(
      fieldProblem(state({ onTop: ["field", "field", "inside"] })),
    ).toBeNull();
    // The centre must still be the field.
    expect(fieldProblem(state({ onTop: ["inside", "field", "field"] }))).toBe(
      "covered",
    );
    expect(fieldProblem(state({ onTop: ["field", "inside", "other"] }))).toBe(
      "covered",
    );
  });

  it("needs IntersectionObserver v2 only in a frame (D63 §1)", () => {
    expect(fieldProblem(state({ visibleV2: false, topFrame: false }))).toBe(
      "not_visible",
    );
    // In the top frame, the hit test and style checks decide.
    expect(fieldProblem(state({ visibleV2: false }))).toBeNull();
    expect(
      fieldProblem(
        state({ visibleV2: false, onTop: ["field", "other", "field"] }),
      ),
    ).toBe("covered");
  });

  it("without v2, fills only in the top frame", () => {
    expect(fieldProblem(state({ visibleV2: undefined }))).toBeNull();
    expect(fieldProblem(state({ visibleV2: undefined, topFrame: false }))).toBe(
      "frame_unchecked",
    );
  });
});

describe("insideField (D65 §3)", () => {
  const field = { left: 100, top: 100, width: 400, height: 40 };

  it("takes a small element wholly inside the field's box", () => {
    // Twitch's show-password button: 36 x 36 at the right end.
    expect(
      insideField(field, { left: 462, top: 102, width: 36, height: 36 }),
    ).toBe(true);
    // Exactly a quarter of the field.
    expect(
      insideField(field, { left: 100, top: 100, width: 100, height: 40 }),
    ).toBe(true);
  });

  it("refuses anything larger, or reaching outside the field", () => {
    // Over the right half.
    expect(
      insideField(field, { left: 300, top: 100, width: 200, height: 40 }),
    ).toBe(false);
    // Small, but past the right edge.
    expect(
      insideField(field, { left: 480, top: 102, width: 36, height: 36 }),
    ).toBe(false);
    // A layer over the whole page (overlay.html).
    expect(
      insideField(field, { left: 0, top: 0, width: 1000, height: 800 }),
    ).toBe(false);
    // Nothing drawn.
    expect(
      insideField(field, { left: 120, top: 110, width: 0, height: 0 }),
    ).toBe(false);
  });
});

describe("unreachable (D65 §1)", () => {
  const open = { ariaHidden: null, inert: false };

  it("is false for a plain field and its ancestors", () => {
    expect(unreachable([open, open, open])).toBe(false);
    expect(
      unreachable([open, { ariaHidden: "false", inert: false }, open]),
    ).toBe(false);
  });

  it("is true under aria-hidden or inert, at any depth", () => {
    // Booking's and Notion's decoy password fields.
    expect(
      unreachable([open, { ariaHidden: "true", inert: false }, open]),
    ).toBe(true);
    expect(unreachable([{ ariaHidden: null, inert: true }])).toBe(true);
    expect(
      unreachable([open, open, { ariaHidden: "TRUE", inert: false }]),
    ).toBe(true);
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

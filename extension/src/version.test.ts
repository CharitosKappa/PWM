import { describe, expect, it } from "vitest";
import { EXTENSION_VERSION } from "./version.ts";

describe("EXTENSION_VERSION", () => {
  it("is a semver string", () => {
    expect(EXTENSION_VERSION).toMatch(/^\d+\.\d+\.\d+$/);
  });
});

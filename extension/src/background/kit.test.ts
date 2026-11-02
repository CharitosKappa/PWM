import { describe, expect, it } from "vitest";
import { localDate, paperFor } from "./kit.ts";

describe("the Emergency Kit's paper", () => {
  it("is Letter in North America and A4 elsewhere", () => {
    expect(paperFor("en-US")).toBe("letter");
    expect(paperFor("en")).toBe("letter");
    expect(paperFor("fr-CA")).toBe("letter");
    expect(paperFor("es-MX")).toBe("letter");
    expect(paperFor("en-GB")).toBe("a4");
    expect(paperFor("el")).toBe("a4");
    expect(paperFor("de-DE")).toBe("a4");
    expect(paperFor("not a locale!")).toBe("a4");
  });

  it("is dated in the local time zone", () => {
    expect(localDate(new Date(2026, 10, 2, 23, 59).getTime())).toBe(
      "2026-11-02",
    );
    expect(localDate(new Date(2026, 0, 9).getTime())).toBe("2026-01-09");
  });
});

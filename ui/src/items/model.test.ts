import { describe as group, expect, test } from "vitest";
import type { ItemView, VaultRow } from "../protocol.ts";
import { REPLIES } from "../protocol-samples.ts";
import {
  addCustomField,
  customFields,
  getIn,
  newItem,
  parseTags,
  removeCustomField,
  setIn,
  spokenCode,
  textAt,
  visibleRows,
} from "./model.ts";

group("screen readers", () => {
  test("a one-time code is read digit by digit in two groups", () => {
    expect(spokenCode("123456")).toBe("1 2 3, 4 5 6");
    expect(spokenCode("12345678")).toBe("1 2 3 4, 5 6 7 8");
    expect(spokenCode("1234")).toBe("1 2 3 4");
  });
});

group("editing an item in place (D18)", () => {
  test("fields this version doesn't know are kept", () => {
    const item = {
      type: "login",
      title: "Mail",
      login: { username: "a", password: "p", future: 1 },
      future_field: { kept: true },
    };
    const edited = setIn(item, ["login", "password"], "new");
    expect(edited).toEqual({
      type: "login",
      title: "Mail",
      login: { username: "a", password: "new", future: 1 },
      future_field: { kept: true },
    });
    // The original isn't changed.
    expect(item.login.password).toBe("p");
  });

  test("a card's fields, and objects made on the way", () => {
    let card = newItem("card");
    card = setIn(card, ["card", "number"], "4111 1111 1111 1234");
    card = setIn(card, ["card", "expiry_month"], 12);
    expect(textAt(card, ["card", "number"])).toBe("4111 1111 1111 1234");
    expect(getIn(card, ["card", "expiry_month"])).toBe(12);
    const made = setIn({}, ["login", "urls", 0, "url"], "https://x.example");
    expect(made).toEqual({ login: { urls: [{ url: "https://x.example" }] } });
  });

  test("custom fields are added, read and removed", () => {
    let item = addCustomField(newItem("note"), "hidden");
    item = setIn(item, ["custom_fields", 0, "name"], "PIN");
    item = addCustomField(item, "text");
    expect(customFields(item)).toEqual([
      { name: "PIN", value: "", kind: "hidden" },
      { name: "", value: "", kind: "text" },
    ]);
    expect(customFields(removeCustomField(item, 0))).toEqual([
      { name: "", value: "", kind: "text" },
    ]);
  });

  test("every type starts as D18 lays it out", () => {
    expect(newItem("login")).toMatchObject({
      type: "login",
      login: { urls: [], totp: null },
    });
    expect(newItem("identity")).toMatchObject({
      identity: { address_lines: [] },
    });
    expect(newItem("note")).not.toHaveProperty("login");
    expect(parseTags(" work, ,home ")).toEqual(["work", "home"]);
  });
});

function row(n: number, extra: Partial<VaultRow> = {}): VaultRow {
  return {
    vaultId: "v",
    itemId: `id-${String(n).padStart(5, "0")}`,
    type: "login",
    title: `Site ${String(n)}`,
    subtitle: `user${String(n)}@example.com`,
    url: `https://site${String(n)}.example.com`,
    favorite: n % 7 === 0,
    folderId: null,
    trashed: false,
    hasPassword: true,
    hasCode: false,
    ...extra,
  };
}

group("the list (D43 §3)", () => {
  const rows = [
    row(1, { title: "beta" }),
    row(2, { title: "Alpha", favorite: true }),
    row(3, { title: "Gamma", trashed: true }),
    row(4, { title: "Visa", type: "card", subtitle: "•••• 1234", url: "" }),
  ];

  test("sections, search and order", () => {
    const titles = (r: VaultRow[]): string[] => r.map((x) => x.title);
    expect(titles(visibleRows(rows, { kind: "all" }, ""))).toEqual([
      "Alpha",
      "beta",
      "Visa",
    ]);
    expect(titles(visibleRows(rows, { kind: "favorites" }, ""))).toEqual([
      "Alpha",
    ]);
    expect(titles(visibleRows(rows, { kind: "trash" }, ""))).toEqual(["Gamma"]);
    expect(
      titles(visibleRows(rows, { kind: "type", type: "card" }, "")),
    ).toEqual(["Visa"]);
    // A card's last 4 digits, a username, a website; trashed items never.
    expect(titles(visibleRows(rows, { kind: "all" }, " 1234 "))).toEqual([
      "Visa",
    ]);
    expect(titles(visibleRows(rows, { kind: "all" }, "USER1@"))).toEqual([
      "beta",
    ]);
    expect(titles(visibleRows(rows, { kind: "all" }, "gamma"))).toEqual([]);
  });

  test("5,000 items filter without a visible delay", () => {
    const many = Array.from({ length: 5000 }, (_, i) => row(i));
    const start = performance.now();
    const found = visibleRows(many, { kind: "all" }, "site42");
    expect(performance.now() - start).toBeLessThan(150);
    expect(found.length).toBe(111);
  });
});

test("no item sample holds a value it marks hidden (D67 §1)", () => {
  const items: ItemView[] = REPLIES.item ?? [];
  expect(items.length).toBeGreaterThan(0);
  for (const view of items) {
    for (const pointer of view.hidden) {
      const path = pointer
        .split("/")
        .slice(1)
        .map((p) => (/^\d+$/.test(p) ? Number(p) : p));
      const value = getIn(view.item, path);
      expect(value === undefined || value === "").toBe(true);
    }
  }
});

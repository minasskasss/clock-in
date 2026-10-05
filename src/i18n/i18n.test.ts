import { describe, expect, it } from "vitest";
import i18n from ".";
import el from "./el.json";

function keyPaths(value: unknown, prefix = ""): string[] {
  if (typeof value !== "object" || value === null) return [prefix];
  return Object.entries(value).flatMap(([key, child]) => keyPaths(child, prefix ? `${prefix}.${key}` : key));
}

describe("i18n", () => {
  it("is Greek only", () => {
    expect(i18n.language).toBe("el");
    expect(Object.keys(i18n.options.resources ?? {})).toEqual(["el"]);
  });

  it("has no empty strings", () => {
    for (const path of keyPaths(el)) {
      const value = path.split(".").reduce<unknown>((node, key) => (node as Record<string, unknown>)[key], el);
      expect(value, path).toEqual(expect.stringMatching(/\S/));
    }
  });
});

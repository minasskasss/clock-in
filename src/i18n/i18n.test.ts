import { describe, expect, it } from "vitest";
import i18n, { DEFAULT_LANGUAGE, setLanguage } from ".";
import el from "./el.json";
import en from "./en.json";

function keyPaths(value: unknown, prefix = ""): string[] {
  if (typeof value !== "object" || value === null) return [prefix];
  return Object.entries(value).flatMap(([key, child]) => keyPaths(child, prefix ? `${prefix}.${key}` : key));
}

describe("i18n", () => {
  it("defaults to Greek", () => {
    expect(DEFAULT_LANGUAGE).toBe("el");
    expect(i18n.options.fallbackLng).toEqual(["el"]);
  });

  it("has the same keys in Greek and English", () => {
    expect(keyPaths(en).sort()).toEqual(keyPaths(el).sort());
  });

  it("has no empty strings", () => {
    for (const resource of [el, en]) {
      for (const path of keyPaths(resource)) {
        const value = path.split(".").reduce<unknown>((node, key) => (node as Record<string, unknown>)[key], resource);
        expect(value, path).toEqual(expect.stringMatching(/\S/));
      }
    }
  });

  it("remembers the chosen language and updates <html lang>", async () => {
    await setLanguage("en");
    expect(window.localStorage.getItem("clockin.language")).toBe("en");
    expect(document.documentElement.lang).toBe("en");
    await setLanguage("el");
    expect(document.documentElement.lang).toBe("el");
  });
});

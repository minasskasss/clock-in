import { describe, expect, it } from "vitest";
import { setSystemPrefersDark } from "../test/setup";
import { isThemePreference, loadThemePreference, resolveTheme, saveThemePreference } from "./theme";

describe("theme", () => {
  it("follows the system by default", () => {
    expect(loadThemePreference()).toBe("system");
  });

  it("resolves 'system' from the OS setting", () => {
    setSystemPrefersDark(false);
    expect(resolveTheme("system")).toBe("light");
    setSystemPrefersDark(true);
    expect(resolveTheme("system")).toBe("dark");
  });

  it("lets a manual choice override the system", () => {
    setSystemPrefersDark(true);
    expect(resolveTheme("light")).toBe("light");
    setSystemPrefersDark(false);
    expect(resolveTheme("dark")).toBe("dark");
  });

  it("persists the preference and ignores invalid stored values", () => {
    saveThemePreference("dark");
    expect(loadThemePreference()).toBe("dark");
    window.localStorage.setItem("clockin.theme", "purple");
    expect(loadThemePreference()).toBe("system");
    expect(isThemePreference("purple")).toBe(false);
  });
});

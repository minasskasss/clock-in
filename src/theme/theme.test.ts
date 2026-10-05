import { describe, expect, it } from "vitest";
import { setSystemPrefersDark } from "../test/setup";
import { isThemePreference, loadThemePreference, rememberAutoDark, resolveTheme, saveThemePreference } from "./theme";

describe("theme", () => {
  it("is automatic by default", () => {
    expect(loadThemePreference()).toBe("auto");
  });

  it("resolves 'auto' from Rust's answer, or the last one before it arrives", () => {
    expect(resolveTheme("auto", true)).toBe("dark");
    expect(resolveTheme("auto", false)).toBe("light");
    expect(resolveTheme("auto")).toBe("light");
    rememberAutoDark(true);
    expect(resolveTheme("auto")).toBe("dark");
  });

  it("resolves 'system' from the OS setting", () => {
    setSystemPrefersDark(false);
    expect(resolveTheme("system", true)).toBe("light");
    setSystemPrefersDark(true);
    expect(resolveTheme("system", false)).toBe("dark");
  });

  it("lets a manual choice override the rest", () => {
    setSystemPrefersDark(true);
    expect(resolveTheme("light", true)).toBe("light");
    setSystemPrefersDark(false);
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("persists the preference and ignores invalid stored values", () => {
    saveThemePreference("dark");
    expect(loadThemePreference()).toBe("dark");
    window.localStorage.setItem("clockin.theme", "purple");
    expect(loadThemePreference()).toBe("auto");
    expect(isThemePreference("purple")).toBe(false);
  });
});

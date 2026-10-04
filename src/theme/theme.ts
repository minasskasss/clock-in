import { readPreference, writePreference } from "../storage";

export const THEME_PREFERENCES = ["system", "light", "dark"] as const;
export type ThemePreference = (typeof THEME_PREFERENCES)[number];
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "clockin.theme";
const DARK_QUERY = "(prefers-color-scheme: dark)";

export function isThemePreference(value: unknown): value is ThemePreference {
  return typeof value === "string" && (THEME_PREFERENCES as readonly string[]).includes(value);
}

/** The saved preference; following the system is the default (SPEC §3). */
export function loadThemePreference(): ThemePreference {
  const value = readPreference(STORAGE_KEY);
  return isThemePreference(value) ? value : "system";
}

export function saveThemePreference(preference: ThemePreference): void {
  writePreference(STORAGE_KEY, preference);
}

function darkQuery(): MediaQueryList | null {
  return typeof window.matchMedia === "function" ? window.matchMedia(DARK_QUERY) : null;
}

export function systemTheme(): ResolvedTheme {
  return darkQuery()?.matches ? "dark" : "light";
}

export function resolveTheme(preference: ThemePreference): ResolvedTheme {
  return preference === "system" ? systemTheme() : preference;
}

/** Sets `data-theme` on <html>; the CSS tokens in `styles/tokens.css` key off it. */
export function applyTheme(theme: ResolvedTheme): void {
  document.documentElement.dataset.theme = theme;
}

/** Calls `listener` whenever the system light/dark setting changes. Returns an unsubscribe function. */
export function onSystemThemeChange(listener: () => void): () => void {
  const query = darkQuery();
  if (!query) return () => {};
  query.addEventListener("change", listener);
  return () => query.removeEventListener("change", listener);
}

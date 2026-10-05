import { readPreference, writePreference } from "../storage";

/**
 * Per-device theme (SPEC §3). "auto" is dark from 21:00 until the business
 * day changes; Rust decides that (`autoDark` in the app state), the UI only
 * applies it.
 */
export const THEME_PREFERENCES = ["auto", "system", "light", "dark"] as const;
export type ThemePreference = (typeof THEME_PREFERENCES)[number];
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "clockin.theme";
/** The last answer of the automatic theme, so a restart at night starts dark. */
const AUTO_STORAGE_KEY = "clockin.theme.autoDark";
const DARK_QUERY = "(prefers-color-scheme: dark)";

export function isThemePreference(value: unknown): value is ThemePreference {
  return typeof value === "string" && (THEME_PREFERENCES as readonly string[]).includes(value);
}

/** The saved preference; "auto" is the default. */
export function loadThemePreference(): ThemePreference {
  const value = readPreference(STORAGE_KEY);
  return isThemePreference(value) ? value : "auto";
}

export function saveThemePreference(preference: ThemePreference): void {
  writePreference(STORAGE_KEY, preference);
}

/** Remembers the automatic theme's latest answer from Rust. */
export function rememberAutoDark(dark: boolean): void {
  writePreference(AUTO_STORAGE_KEY, dark ? "dark" : "light");
}

function lastAutoTheme(): ResolvedTheme {
  return readPreference(AUTO_STORAGE_KEY) === "dark" ? "dark" : "light";
}

function darkQuery(): MediaQueryList | null {
  return typeof window.matchMedia === "function" ? window.matchMedia(DARK_QUERY) : null;
}

export function systemTheme(): ResolvedTheme {
  return darkQuery()?.matches ? "dark" : "light";
}

/**
 * The theme to show. `autoDark` is Rust's answer for "auto", or null before
 * the first answer arrives (the last remembered one is used then).
 */
export function resolveTheme(preference: ThemePreference, autoDark: boolean | null = null): ResolvedTheme {
  switch (preference) {
    case "auto":
      return autoDark === null ? lastAutoTheme() : autoDark ? "dark" : "light";
    case "system":
      return systemTheme();
    default:
      return preference;
  }
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

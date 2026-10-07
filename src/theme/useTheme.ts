import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import {
  applyTheme,
  loadThemePreference,
  onSystemThemeChange,
  rememberAutoDark,
  resolveTheme,
  saveThemePreference,
  type ThemePreference,
} from "./theme";

/**
 * Current theme preference, kept applied to the document. `autoDark` comes
 * from the app state poll, so "auto" switches live at 21:00 and at the
 * rollover.
 */
export function useTheme(autoDark: boolean | null): [ThemePreference, (preference: ThemePreference) => void] {
  const [preference, setPreferenceState] = useState<ThemePreference>(loadThemePreference);

  useEffect(() => {
    if (autoDark !== null) rememberAutoDark(autoDark);
  }, [autoDark]);

  // Rust passes the choice on to Android's native alarm screen. Sent once
  // the app core answers (autoDark arrives with the first state) and on
  // every change.
  const ready = autoDark !== null;
  useEffect(() => {
    if (ready) void api.setTheme(preference).catch(() => {});
  }, [preference, ready]);

  useEffect(() => {
    applyTheme(resolveTheme(preference, autoDark));
    if (preference !== "system") return;
    return onSystemThemeChange(() => applyTheme(resolveTheme("system")));
  }, [preference, autoDark]);

  const setPreference = useCallback((next: ThemePreference) => {
    saveThemePreference(next);
    setPreferenceState(next);
  }, []);

  return [preference, setPreference];
}

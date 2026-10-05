import { useCallback, useEffect, useState } from "react";
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

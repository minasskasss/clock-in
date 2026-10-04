import { useCallback, useEffect, useState } from "react";
import {
  applyTheme,
  loadThemePreference,
  onSystemThemeChange,
  resolveTheme,
  saveThemePreference,
  type ThemePreference,
} from "./theme";

/** Current theme preference, kept applied to the document. */
export function useTheme(): [ThemePreference, (preference: ThemePreference) => void] {
  const [preference, setPreferenceState] = useState<ThemePreference>(loadThemePreference);

  useEffect(() => {
    applyTheme(resolveTheme(preference));
    if (preference !== "system") return;
    return onSystemThemeChange(() => applyTheme(resolveTheme("system")));
  }, [preference]);

  const setPreference = useCallback((next: ThemePreference) => {
    saveThemePreference(next);
    setPreferenceState(next);
  }, []);

  return [preference, setPreference];
}

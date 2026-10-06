import { getCurrentWindow } from "@tauri-apps/api/window";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./i18n";
import "./styles/global.css";
import App from "./App";
import { AlarmApp } from "./alarm/AlarmApp";
import { applyTheme, loadThemePreference, resolveTheme } from "./theme/theme";

// Apply the theme before the first render to avoid a flash of the wrong palette
// ("auto" uses its last known answer until Rust's first one arrives).
applyTheme(resolveTheme(loadThemePreference()));

/** Rust opens the same page in the main window and the alarm window. */
function windowLabel(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
}

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>{windowLabel() === "alarm" ? <AlarmApp /> : <App />}</StrictMode>,
);

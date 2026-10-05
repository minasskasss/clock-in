import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./i18n";
import "./styles/global.css";
import App from "./App";
import { applyTheme, loadThemePreference, resolveTheme } from "./theme/theme";

// Apply the theme before the first render to avoid a flash of the wrong palette
// ("auto" uses its last known answer until Rust's first one arrives).
applyTheme(resolveTheme(loadThemePreference()));

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

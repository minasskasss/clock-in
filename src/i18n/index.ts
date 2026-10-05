import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import el from "./el.json";

/**
 * The app is Greek only (SPEC §3). Every user-facing string lives in
 * `el.json`, so the wording can be changed in one place.
 */
void i18n.use(initReactI18next).init({
  resources: {
    el: { translation: el },
  },
  lng: "el",
  fallbackLng: "el",
  supportedLngs: ["el"],
  interpolation: {
    // React already escapes rendered values.
    escapeValue: false,
  },
  returnNull: false,
});

export default i18n;

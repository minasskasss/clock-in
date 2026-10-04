import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { readPreference, writePreference } from "../storage";
import el from "./el.json";
import en from "./en.json";

export const LANGUAGES = ["el", "en"] as const;
export type Language = (typeof LANGUAGES)[number];

/** Greek is the default language (SPEC §3). */
export const DEFAULT_LANGUAGE: Language = "el";

/** Each language is always shown by its own name, whatever the UI language is. */
export const LANGUAGE_NAMES: Record<Language, string> = {
  el: "Ελληνικά",
  en: "English",
};

const STORAGE_KEY = "clockin.language";

export function isLanguage(value: unknown): value is Language {
  return typeof value === "string" && (LANGUAGES as readonly string[]).includes(value);
}

function storedLanguage(): Language {
  const value = readPreference(STORAGE_KEY);
  return isLanguage(value) ? value : DEFAULT_LANGUAGE;
}

export function setLanguage(language: Language): Promise<unknown> {
  writePreference(STORAGE_KEY, language);
  return i18n.changeLanguage(language);
}

i18n.on("languageChanged", (language) => {
  document.documentElement.lang = language;
});

void i18n.use(initReactI18next).init({
  resources: {
    el: { translation: el },
    en: { translation: en },
  },
  lng: storedLanguage(),
  fallbackLng: DEFAULT_LANGUAGE,
  supportedLngs: LANGUAGES,
  interpolation: {
    // React already escapes rendered values.
    escapeValue: false,
  },
  returnNull: false,
});

export default i18n;

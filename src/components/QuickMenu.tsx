import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { LANGUAGE_NAMES, LANGUAGES, isLanguage, setLanguage } from "../i18n";
import { api, type AppStateView } from "../api";
import { THEME_PREFERENCES, type ThemePreference } from "../theme/theme";
import "./QuickMenu.css";

interface QuickMenuProps {
  theme: ThemePreference;
  onThemeChange: (theme: ThemePreference) => void;
  /** Debug builds only: profile and fake clock. */
  debug?: AppStateView["debug"];
  onDebugChange?: () => void;
}

/** Language and theme menu. Needs no passphrase (SPEC §3). */
export function QuickMenu({ theme, onThemeChange, debug, onDebugChange }: QuickMenuProps) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const panelId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const currentLanguage = isLanguage(i18n.resolvedLanguage) ? i18n.resolvedLanguage : undefined;

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        buttonRef.current?.focus();
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div className="quick-menu" ref={rootRef}>
      <button
        ref={buttonRef}
        type="button"
        className="quick-menu__button"
        aria-label={open ? t("menu.close") : t("menu.open")}
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen((value) => !value)}
      >
        <MenuIcon />
      </button>
      {open && (
        <div id={panelId} className="quick-menu__panel">
          <fieldset className="quick-menu__group">
            <legend className="quick-menu__legend">{t("menu.language")}</legend>
            <div className="segmented">
              {LANGUAGES.map((language) => (
                <button
                  key={language}
                  type="button"
                  lang={language}
                  className="segmented__option"
                  aria-pressed={currentLanguage === language}
                  onClick={() => void setLanguage(language)}
                >
                  {LANGUAGE_NAMES[language]}
                </button>
              ))}
            </div>
          </fieldset>
          <fieldset className="quick-menu__group">
            <legend className="quick-menu__legend">{t("menu.theme")}</legend>
            <div className="segmented">
              {THEME_PREFERENCES.map((option) => (
                <button
                  key={option}
                  type="button"
                  className="segmented__option"
                  aria-pressed={theme === option}
                  onClick={() => onThemeChange(option)}
                >
                  {t(`theme.${option}`)}
                </button>
              ))}
            </div>
          </fieldset>
          {debug && <DebugSection debug={debug} onChange={onDebugChange} />}
        </div>
      )}
    </div>
  );
}

/** Debug builds: which profile this is, and the fake clock (ARCHITECTURE §9). */
function DebugSection({ debug, onChange }: { debug: NonNullable<AppStateView["debug"]>; onChange?: () => void }) {
  const { t } = useTranslation();
  const [value, setValue] = useState(debug.fakeClock ?? "");
  const set = (local: string | null) =>
    void api
      .debugSetClock(local)
      .then(() => onChange?.())
      .catch(() => {});
  return (
    <fieldset className="quick-menu__group quick-menu__debug">
      <legend className="quick-menu__legend">{t("menu.debug")}</legend>
      {debug.profile && <p className="quick-menu__note">{t("menu.profile", { name: debug.profile })}</p>}
      <label className="quick-menu__note" htmlFor="fake-clock">
        {t("menu.fakeClock")}
      </label>
      <input id="fake-clock" className="input" type="datetime-local" value={value} onChange={(e) => setValue(e.target.value)} />
      <div className="segmented">
        <button type="button" className="segmented__option" onClick={() => set(value || null)} disabled={!value}>
          {t("menu.setClock")}
        </button>
        <button type="button" className="segmented__option" onClick={() => set(null)}>
          {t("menu.realClock")}
        </button>
      </div>
      {debug.fakeClock && <p className="quick-menu__note">{t("menu.fakeClockOn", { time: debug.fakeClock.replace("T", " ") })}</p>}
    </fieldset>
  );
}

function MenuIcon() {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path d="M4 7h16M4 12h16M4 17h16" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

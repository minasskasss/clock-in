import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type AlertMode, type AndroidView, type AppStateView, type Platform } from "../api";
import { formatDateTime, parseDateTime } from "../dates";
import { THEME_PREFERENCES, type ThemePreference } from "../theme/theme";
import "./QuickMenu.css";

interface QuickMenuProps {
  theme: ThemePreference;
  onThemeChange: (theme: ThemePreference) => void;
  platform: Platform;
  /** Android, paired: the alert mode and the permission checklist. */
  android?: AndroidView | null;
  onAndroidChange?: () => void;
  onOpenPermissions?: () => void;
  /** Debug builds only: profile and fake clock. */
  debug?: AppStateView["debug"];
  onDebugChange?: () => void;
}

const ALERT_MODES: AlertMode[] = ["ring", "notification"];

/**
 * Theme menu; on Android also the alert mode and the permission checklist
 * (and the debug tools in debug builds). Needs no passphrase (SPEC §3, §8.2).
 */
export function QuickMenu({
  theme,
  onThemeChange,
  platform,
  android,
  onAndroidChange,
  onOpenPermissions,
  debug,
  onDebugChange,
}: QuickMenuProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const panelId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

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
            <legend className="quick-menu__legend">{t("menu.theme")}</legend>
            <div className="segmented segmented--grid">
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
            <p className="quick-menu__note">
              {t(theme === "system" && platform === "android" ? "theme.systemHintAndroid" : `theme.${theme}Hint`)}
            </p>
          </fieldset>
          {android && (
            <fieldset className="quick-menu__group">
              <legend className="quick-menu__legend">{t("alertMode.title")}</legend>
              <div className="segmented">
                {ALERT_MODES.map((mode) => (
                  <button
                    key={mode}
                    type="button"
                    className="segmented__option"
                    aria-pressed={android.alertMode === mode}
                    onClick={() => void api.setAlertMode(mode).then(onAndroidChange, () => {})}
                  >
                    {t(`alertMode.${mode}`)}
                  </button>
                ))}
              </div>
              <p className="quick-menu__note">{t(`alertMode.${android.alertMode}Hint`)}</p>
              <button
                type="button"
                className="button button--quiet"
                onClick={() => {
                  setOpen(false);
                  onOpenPermissions?.();
                }}
              >
                {android.permissionsOk ? "✓" : "✗"} {t("menu.permissions")}
              </button>
            </fieldset>
          )}
          {debug && <DebugSection debug={debug} onChange={onDebugChange} />}
        </div>
      )}
    </div>
  );
}

/** Debug builds: which profile this is, and the fake clock (ARCHITECTURE §9). */
function DebugSection({ debug, onChange }: { debug: NonNullable<AppStateView["debug"]>; onChange?: () => void }) {
  const { t } = useTranslation();
  const [value, setValue] = useState(debug.fakeClock ? formatDateTime(debug.fakeClock) : "");
  const [second, setSecond] = useState(debug.fakeClockSecond);
  const [bad, setBad] = useState(false);
  const set = (local: string | null) =>
    void api.debugSetClock(local, local !== null && second).then(
      () => {
        setBad(false);
        onChange?.();
      },
      () => setBad(true),
    );
  return (
    <fieldset className="quick-menu__group quick-menu__debug">
      <legend className="quick-menu__legend">{t("menu.debug")}</legend>
      {debug.profile && <p className="quick-menu__note">{t("menu.profile", { name: debug.profile })}</p>}
      <label className="quick-menu__note" htmlFor="fake-clock">
        {t("menu.fakeClock")}
      </label>
      <input
        id="fake-clock"
        className="input"
        placeholder={t("menu.fakeClockPlaceholder")}
        autoComplete="off"
        value={value}
        onChange={(e) => setValue(e.target.value)}
      />
      {bad && <p className="field__error">{t("menu.fakeClockFormat")}</p>}
      <label className="checkbox quick-menu__note">
        <input type="checkbox" checked={second} onChange={(e) => setSecond(e.target.checked)} />
        {t("menu.fakeClockSecond")}
      </label>
      <div className="segmented">
        <button
          type="button"
          className="segmented__option"
          onClick={() => {
            const local = parseDateTime(value);
            if (local) set(local);
            else setBad(true);
          }}
          disabled={!value}
        >
          {t("menu.setClock")}
        </button>
        <button type="button" className="segmented__option" onClick={() => set(null)}>
          {t("menu.realClock")}
        </button>
      </div>
      {debug.fakeClock && (
        <p className="quick-menu__note">
          {t(debug.fakeClockSecond ? "menu.fakeClockOnSecond" : "menu.fakeClockOn", {
            time: formatDateTime(debug.fakeClock),
          })}
        </p>
      )}
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

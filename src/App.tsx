import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import "./App.css";
import { api } from "./api";
import { Background } from "./components/Background";
import { Brand } from "./components/Brand";
import { Diagnostics } from "./components/Diagnostics";
import { FirstRun } from "./components/FirstRun";
import { Onboarding } from "./components/Onboarding";
import { QuitDialog } from "./components/QuitDialog";
import { QuickMenu } from "./components/QuickMenu";
import { Today } from "./components/Today";
import { Unlock } from "./components/Unlock";
import { Settings } from "./settings/Settings";
import { readPreference, writePreference } from "./storage";
import { useTheme } from "./theme/useTheme";
import { useAppState } from "./useAppState";

/** Sent by Rust when Quit is chosen in the tray (src-tauri/src/tray.rs). */
export const QUIT_REQUESTED = "quit-requested";

/** Android: the onboarding checklist was finished or put off once on this phone. */
const ONBOARDING_KEY = "clockin.onboarding";

export default function App() {
  const { t } = useTranslation();
  const { state, failed, refresh } = useAppState();
  const [theme, setTheme] = useTheme(state?.autoDark ?? null);
  const [screen, setScreen] = useState<"today" | "settings">("today");
  // Android: «Διαγνωστικά» from the menu, over any screen.
  const [diagnostics, setDiagnostics] = useState(false);
  const [unlocking, setUnlocking] = useState(false);
  const [quitting, setQuitting] = useState(false);
  // Android: the checklist opens by itself after pairing, until finished or
  // put off once; later it is in the menu and behind the Today banner.
  const [onboarding, setOnboarding] = useState(() => readPreference(ONBOARDING_KEY) !== "done");

  // `data-platform` on <html> lets the CSS give Android phones their own sizes.
  const platform = state?.platform;
  useEffect(() => {
    if (platform) document.documentElement.dataset.platform = platform;
  }, [platform]);

  // Android draws edge to edge, and older WebViews report no safe-area
  // insets: the bars' sizes come from Android too (App.css uses the larger).
  useEffect(() => {
    if (platform !== "android") return;
    const apply = () =>
      void api.androidInsets().then(
        (insets) => {
          if (!insets) return;
          for (const side of ["top", "right", "bottom", "left"] as const) {
            document.documentElement.style.setProperty(`--android-inset-${side}`, `${insets[side]}px`);
          }
        },
        () => {},
      );
    apply();
    window.addEventListener("resize", apply);
    return () => window.removeEventListener("resize", apply);
  }, [platform]);

  // Tray → Quit (Rust shows this window first, then asks for the quit code).
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let alive = true;
    listen(QUIT_REQUESTED, () => setQuitting(true)).then(
      (stop) => (alive ? (unlisten = stop) : stop()),
      () => {},
    );
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  const paired = state?.phase === "paired";
  const android = state?.android ?? null;
  const showOnboarding = paired && android !== null && onboarding;
  const finishOnboarding = () => {
    writePreference(ONBOARDING_KEY, "done");
    setOnboarding(false);
  };
  // Settings show only while unlocked: idle time or unpairing locks them.
  const inSettings = paired && screen === "settings" && state.adminUnlocked;

  const openSettings = () => {
    if (state?.adminUnlocked) setScreen("settings");
    else setUnlocking(true);
  };

  const exitSettings = () => {
    void api.adminLogout().catch(() => {});
    setScreen("today");
    void refresh();
  };

  let main;
  if (diagnostics && state?.platform === "android") {
    main = <Diagnostics onClose={() => setDiagnostics(false)} />;
  } else if (!state) {
    main = <p className="app__message">{failed ? t("app.coreFailed") : t("app.loading")}</p>;
  } else if (state.phase === "not_configured") {
    main = (
      <section className="card app__notice">
        <h1 className="card__title">{t("notConfigured.title")}</h1>
        <p className="card__body">{t("notConfigured.body")}</p>
      </section>
    );
  } else if (state.phase === "unpaired") {
    main = (
      <FirstRun
        defaultDeviceName={state.defaultDeviceName}
        lockoutRemainingS={state.lockoutRemainingS}
        onDone={() => {
          setScreen("today");
          void refresh();
        }}
      />
    );
  } else if (showOnboarding) {
    main = <Onboarding permissions={android.permissions} permissionsOk={android.permissionsOk} onDone={finishOnboarding} />;
  } else if (inSettings) {
    main = (
      <Settings
        dataVersion={state.dataVersion}
        lockoutRemainingS={state.lockoutRemainingS}
        platform={state.platform}
        onExit={exitSettings}
      />
    );
  } else {
    main = (
      <Today
        today={state.today}
        banners={state.banners}
        platform={state.platform}
        permissionsMissing={android !== null && !android.permissionsOk}
        onFixPermissions={() => setOnboarding(true)}
        alarm={android?.alarm ?? null}
        onStopAlarm={() => void api.androidStopAlarm().then(refresh, () => {})}
        onChanged={() => void refresh()}
      />
    );
  }

  return (
    <>
      <Background />
      <div className="app">
        <header className="app__header">
          <div className="app__brand">
            <Brand />
            {state?.environment === "dev" && <span className="env-badge">{t("app.devBadge")}</span>}
          </div>
          <div className="app__actions">
            {paired && !inSettings && !showOnboarding && (
              <button type="button" className="round-button" aria-label={t("today.settings")} title={t("today.settings")} onClick={openSettings}>
                <GearIcon />
              </button>
            )}
            <QuickMenu
              theme={theme}
              onThemeChange={setTheme}
              platform={state?.platform ?? "windows"}
              android={paired ? android : null}
              onAndroidChange={() => void refresh()}
              onOpenPermissions={() => {
                setDiagnostics(false);
                setOnboarding(true);
              }}
              onOpenDiagnostics={() => setDiagnostics(true)}
              debug={state?.debug ?? null}
              onDebugChange={() => void refresh()}
            />
          </div>
        </header>
        <main className="app__main">{main}</main>
      </div>
      {unlocking && state && (
        <Unlock
          mode="open"
          lockoutRemainingS={state.lockoutRemainingS}
          onUnlocked={() => {
            setUnlocking(false);
            setScreen("settings");
            void refresh();
          }}
          onCancel={() => setUnlocking(false)}
        />
      )}
      {quitting && <QuitDialog codeSet={state?.quitCodeSet ?? false} onCancel={() => setQuitting(false)} />}
    </>
  );
}

function GearIcon() {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M12 15.2a3.2 3.2 0 1 0 0-6.4 3.2 3.2 0 0 0 0 6.4Z"
        stroke="currentColor"
        strokeWidth="1.8"
      />
      <path
        d="M19.4 13.5a7.6 7.6 0 0 0 0-3l2-1.6-2-3.4-2.4.9a7.4 7.4 0 0 0-2.6-1.5L14 2.4h-4l-.4 2.5A7.4 7.4 0 0 0 7 6.4l-2.4-.9-2 3.4 2 1.6a7.6 7.6 0 0 0 0 3l-2 1.6 2 3.4 2.4-.9a7.4 7.4 0 0 0 2.6 1.5l.4 2.5h4l.4-2.5a7.4 7.4 0 0 0 2.6-1.5l2.4.9 2-3.4-2-1.6Z"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinejoin="round"
      />
    </svg>
  );
}

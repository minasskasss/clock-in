import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, isNoSession, type AdminView } from "../api";
import { FormMessage } from "../components/Field";
import { Unlock } from "../components/Unlock";
import { errorMessage } from "../errors";
import { AdminContext, type Admin } from "./admin";
import { AlarmsPanel } from "./AlarmsPanel";
import { DevicesPanel } from "./DevicesPanel";
import { MarksPanel } from "./MarksPanel";
import { OverridesPanel } from "./OverridesPanel";
import { PassphrasePanel } from "./PassphrasePanel";
import { QuitCodePanel } from "./QuitCodePanel";
import { StaffPanel } from "./StaffPanel";
import "./Settings.css";

const TABS = ["staff", "overrides", "marks", "alarms", "devices", "codes"] as const;
type Tab = (typeof TABS)[number];

interface SettingsProps {
  /** Changes whenever server data changed; the lists reload. */
  dataVersion: number;
  lockoutRemainingS: number;
  /** Leave Settings (they lock). */
  onExit: () => void;
}

/** The passphrase-protected Settings (SPEC §4). */
export function Settings({ dataVersion, lockoutRemainingS, onExit }: SettingsProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<Tab>("staff");
  const [view, setView] = useState<AdminView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [reauth, setReauth] = useState<((ok: boolean) => void) | null>(null);

  const reload = useCallback(
    () =>
      api.adminView().then(
        (next) => {
          setView(next);
          setLoadError(null);
        },
        (e: unknown) => {
          if (!isNoSession(e)) setLoadError(errorMessage(t, e));
        },
      ),
    [t],
  );

  // Reload whenever the server data changed (another device, or our own save).
  useEffect(() => {
    let current = true;
    api.adminView().then(
      (next) => {
        if (!current) return;
        setView(next);
        setLoadError(null);
      },
      (e: unknown) => {
        if (current && !isNoSession(e)) setLoadError(errorMessage(t, e));
      },
    );
    return () => {
      current = false;
    };
  }, [dataVersion, t]);

  const askPassphrase = useCallback(
    () => new Promise<boolean>((resolve) => setReauth(() => resolve)),
    [],
  );

  const run = useCallback(
    async <T,>(action: () => Promise<T>): Promise<T> => {
      try {
        const result = await action();
        void reload();
        return result;
      } catch (e) {
        if (!isNoSession(e) || !(await askPassphrase())) throw e;
        const result = await action();
        void reload();
        return result;
      }
    },
    [reload, askPassphrase],
  );

  const admin: Admin | null = useMemo(() => (view ? { view, run, reload } : null), [view, run, reload]);

  // Any use of Settings keeps them unlocked; 5 idle minutes lock them (Rust decides).
  const touch = () => void api.adminTouch().catch(() => {});

  return (
    <section className="settings" aria-labelledby="settings-title" onPointerDown={touch} onKeyDown={touch}>
      <div className="settings__bar">
        <button type="button" className="button button--quiet" onClick={onExit}>
          ← {t("settings.back")}
        </button>
        <h1 id="settings-title" className="settings__title">
          {t("settings.title")}
        </h1>
      </div>
      <div className="tabs" role="tablist" aria-label={t("settings.title")}>
        {TABS.map((key) => (
          <button
            key={key}
            type="button"
            role="tab"
            id={`tab-${key}`}
            aria-selected={tab === key}
            aria-controls={`panel-${key}`}
            className="tabs__tab"
            onClick={() => setTab(key)}
          >
            {t(`settings.tabs.${key}`)}
          </button>
        ))}
      </div>
      <div role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`} className="settings__panel card">
        {loadError && <FormMessage tone="error">{loadError}</FormMessage>}
        {admin ? (
          <AdminContext.Provider value={admin}>
            {tab === "staff" && <StaffPanel />}
            {tab === "overrides" && <OverridesPanel />}
            {tab === "marks" && <MarksPanel />}
            {tab === "alarms" && <AlarmsPanel />}
            {tab === "devices" && <DevicesPanel />}
            {tab === "codes" && (
              <div className="panel-stack">
                <PassphrasePanel lockoutRemainingS={lockoutRemainingS} onChanged={onExit} />
                <QuitCodePanel />
              </div>
            )}
          </AdminContext.Provider>
        ) : (
          !loadError && <p className="panel__empty">{t("app.loading")}</p>
        )}
      </div>
      {reauth && (
        <Unlock
          mode="reauth"
          lockoutRemainingS={lockoutRemainingS}
          onUnlocked={() => {
            reauth(true);
            setReauth(null);
          }}
          onCancel={() => {
            reauth(false);
            setReauth(null);
          }}
        />
      )}
    </section>
  );
}

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type DiagnosticsView, type LocalStamp } from "../api";
import { formatDate } from "../dates";
import "./Diagnostics.css";

/** How often the open view reads the phone again. */
const REFRESH_MS = 3000;

const PERMISSIONS = ["notifications", "exactAlarms", "fullScreen", "battery", "unusedApps"] as const;

const HOW_KEYS = {
  fullScreen: "diagnostics.howFullScreen",
  opened: "diagnostics.howOpened",
  notification: "diagnostics.howNotification",
  notificationMode: "diagnostics.howNotificationMode",
} as const;

function stamp(value: LocalStamp | null): string | null {
  return value ? `${formatDate(value.date)} ${value.time}` : null;
}

/**
 * «Διαγνωστικά» (Android): read-only, plain Greek, no secrets. Everything
 * needed to set up and check a phone remotely, in one screenshot.
 */
export function Diagnostics({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const [view, setView] = useState<DiagnosticsView | null>(null);

  useEffect(() => {
    let alive = true;
    const read = () =>
      api.androidDiagnostics().then(
        (next) => alive && setView(next),
        () => {},
      );
    void read();
    const timer = window.setInterval(() => void read(), REFRESH_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  const rows: { label: string; value: string; bad?: boolean }[] = [];
  if (view) {
    const p = view.permissions;
    rows.push(
      {
        label: t("diagnostics.app"),
        value: t("diagnostics.appValue", {
          version: view.appVersion,
          env: t(view.environment === "dev" ? "diagnostics.envDev" : "diagnostics.envProd"),
        }),
      },
      { label: t("diagnostics.phone"), value: view.phone },
      {
        label: t("diagnostics.android"),
        value: t("diagnostics.androidValue", { version: view.androidVersion, sdk: view.sdk }),
      },
    );
    if (view.makerOs) rows.push({ label: t("diagnostics.makerOs"), value: view.makerOs });
    rows.push({
      label: t("diagnostics.webView"),
      value: !view.webViewVersion
        ? t("diagnostics.webViewUnknown")
        : view.webViewOk
          ? view.webViewVersion
          : t("diagnostics.webViewOld", { version: view.webViewVersion }),
      bad: !view.webViewOk,
    });
    rows.push({ label: t("diagnostics.alertMode"), value: t(`alertMode.${view.alertMode}`) });
    if (p) {
      for (const kind of PERMISSIONS) {
        if (p.notApplicable.includes(kind)) {
          rows.push({ label: t(`onboarding.${kind}`), value: t("diagnostics.notApplicable") });
        } else {
          rows.push({ label: t(`onboarding.${kind}`), value: p[kind] ? "✓" : "✗", bad: !p[kind] });
        }
      }
      rows.push({
        label: t("diagnostics.oem"),
        value:
          p.oem === ""
            ? t("diagnostics.oemNone")
            : p.oemDone
              ? t("diagnostics.oemDone")
              : t("diagnostics.oemNotDone"),
        bad: p.oem !== "" && !p.oemDone,
      });
    }
    const refresh = stamp(view.lastRefresh);
    rows.push(
      { label: t("diagnostics.lastSync"), value: stamp(view.lastSync) ?? t("diagnostics.never") },
      {
        label: t("diagnostics.lastRefresh"),
        value: refresh
          ? `${refresh}, ${t(view.lastRefreshOk ? "diagnostics.refreshOk" : "diagnostics.refreshFailed")}`
          : t("diagnostics.never"),
        bad: refresh !== null && !view.lastRefreshOk,
      },
      { label: t("diagnostics.nextAlarm"), value: stamp(view.nextAlarm) ?? t("diagnostics.none") },
      {
        label: t("diagnostics.lastAlarm"),
        value:
          view.lastAlarm && view.lastAlarmHow
            ? `${stamp(view.lastAlarm)}, ${t(HOW_KEYS[view.lastAlarmHow])}`
            : t("diagnostics.none"),
        bad: view.lastAlarmHow === "notification",
      },
    );
  }

  return (
    <section className="diagnostics card" aria-labelledby="diagnostics-title">
      <h1 id="diagnostics-title" className="diagnostics__title">
        {t("diagnostics.title")}
      </h1>
      <p className="diagnostics__intro">{t("diagnostics.intro")}</p>
      {view === null ? (
        <p className="diagnostics__intro">{t("app.loading")}</p>
      ) : (
        <dl className="diagnostics__list">
          {rows.map((row) => (
            <div key={row.label} className={`diagnostics__row${row.bad ? " diagnostics__row--bad" : ""}`}>
              <dt>{row.label}</dt>
              <dd>{row.value}</dd>
            </div>
          ))}
        </dl>
      )}
      <div className="diagnostics__actions">
        <button type="button" className="button" onClick={onClose}>
          {t("diagnostics.back")}
        </button>
      </div>
    </section>
  );
}

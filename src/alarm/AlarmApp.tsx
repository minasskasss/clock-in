import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import "../App.css";
import { api, type AlarmStateView } from "../api";
import { Background } from "../components/Background";
import { useTheme } from "../theme/useTheme";
import "./AlarmApp.css";

/** How often the alarm window asks Rust what to show. */
export const ALARM_POLL_MS = 500;

/**
 * The alarm window (SPEC §7.3, ARCHITECTURE §9): the names under "Άφιξη" and
 * "Αποχώρηση" and one large Stop button. Rust opens it on top of everything,
 * plays the sound and closes it once Stop is pressed or nobody is left.
 * Stop is not focused on purpose: a key pressed while typing elsewhere must
 * not silence the alarm.
 */
export function AlarmApp() {
  const { t } = useTranslation();
  const [state, setState] = useState<AlarmStateView | null>(null);
  const [stopped, setStopped] = useState<number | null>(null);
  useTheme(state?.autoDark ?? null);

  useEffect(() => {
    let alive = true;
    const load = () =>
      api.alarmState().then(
        (next) => alive && setState(next),
        () => {},
      );
    void load();
    const timer = window.setInterval(() => void load(), ALARM_POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  const alarm = state?.alarm ?? null;
  const stop = () => {
    if (!alarm) return;
    setStopped(alarm.id);
    // A failed call leaves the button usable again.
    void api.alarmStop(alarm.id).catch(() => setStopped(null));
  };

  return (
    <>
      <Background />
      <main className={`alarm${alarm?.ringing ? " alarm--ringing" : ""}`}>
        {alarm && (
          <>
            <header className="alarm__head">
              <p className="alarm__title">{t("alarm.title")}</p>
              <p className="alarm__time">{alarm.at}</p>
            </header>
            <div className="alarm__lists">
              <NameList title={t("alarm.checkIn")} names={alarm.checkIn} kind="in" />
              <NameList title={t("alarm.checkOut")} names={alarm.checkOut} kind="out" />
            </div>
            {!alarm.ringing && alarm.reringAt && (
              <p className="alarm__silent" role="status">
                {t("alarm.silent", { time: alarm.reringAt })}
              </p>
            )}
            <button type="button" className="button alarm__stop" onClick={stop} disabled={stopped === alarm.id}>
              {t("alarm.stop")}
            </button>
            <p className="alarm__note">{t("alarm.note")}</p>
          </>
        )}
      </main>
    </>
  );
}

function NameList({ title, names, kind }: { title: string; names: string[]; kind: "in" | "out" }) {
  if (names.length === 0) return null;
  return (
    <section className={`alarm__list alarm__list--${kind}`} aria-label={title}>
      <h2 className="alarm__list-title">{title}</h2>
      <ul className="alarm__names">
        {names.map((name, index) => (
          <li key={`${index}-${name}`}>{name}</li>
        ))}
      </ul>
    </section>
  );
}

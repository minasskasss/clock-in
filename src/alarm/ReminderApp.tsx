import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import "../App.css";
import { api, type ReminderLine, type ReminderStateView } from "../api";
import { Background } from "../components/Background";
import { Dialog } from "../components/Dialog";
import { FormMessage } from "../components/Field";
import { errorMessage } from "../errors";
import { useTheme } from "../theme/useTheme";
import "./AlarmApp.css";
import "./ReminderApp.css";

/** How often the reminder window asks Rust what to show. */
export const REMINDER_POLL_MS = 500;

/**
 * The «Λείπει σήμανση» reminder window (SPEC §7.6): after «Σταμάτημα», the
 * names of that alarm still not marked, each with its own time and a
 * «Σήμανση» button (with the same confirmation as the Today list), and
 * «Εντάξει», which ends the reminders on this device. Rust opens it on top,
 * plays the C and closes it once «Εντάξει» is pressed, nobody is left, or the
 * business day ends.
 */
export function ReminderApp() {
  const { t } = useTranslation();
  const [state, setState] = useState<ReminderStateView | null>(null);
  const [confirm, setConfirm] = useState<ReminderLine | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [acknowledged, setAcknowledged] = useState<string | null>(null);
  useTheme(state?.autoDark ?? null);

  useEffect(() => {
    let alive = true;
    const load = () =>
      api.reminderState().then(
        (next) => alive && setState(next),
        () => {},
      );
    void load();
    const timer = window.setInterval(() => void load(), REMINDER_POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  const reminder = state?.reminder ?? null;
  const groups = reminder ? reminder.groupIds.join(",") : null;

  const ok = () => {
    if (!reminder) return;
    setAcknowledged(groups);
    // A failed call leaves the button usable again.
    void api.reminderOk(reminder.groupIds).catch(() => setAcknowledged(null));
  };

  const submit = async () => {
    if (!confirm) return;
    setBusy(true);
    try {
      await api.mark(confirm, confirm.kind);
      setConfirm(null);
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  };

  const ask = (line: ReminderLine) => {
    setError(null);
    setConfirm(line);
  };

  return (
    <>
      <Background />
      <main className="alarm reminder">
        {reminder && (
          <>
            <header className="alarm__head">
              <p className="alarm__title">{t("reminder.title")}</p>
            </header>
            <p className="reminder__intro">{t("reminder.intro")}</p>
            <div className="alarm__lists">
              <Lines title={t("reminder.checkIn")} lines={reminder.checkIn} kind="in" onMark={ask} />
              <Lines title={t("reminder.checkOut")} lines={reminder.checkOut} kind="out" onMark={ask} />
            </div>
            <p className="reminder__next" role="status">
              {reminder.nextAt ? t("reminder.next", { time: reminder.nextAt }) : t("reminder.last")}
            </p>
            <button type="button" className="button button--primary reminder__ok" onClick={ok} disabled={acknowledged === groups}>
              {t("reminder.ok")}
            </button>
            <p className="alarm__note">{t("reminder.note")}</p>
          </>
        )}
      </main>
      {confirm && (
        <Dialog
          title={confirm.kind === "in" ? t("confirm.inTitle") : t("confirm.outTitle")}
          onClose={() => !busy && setConfirm(null)}
          actions={
            <>
              <button type="button" className="button button--quiet button--large" onClick={() => setConfirm(null)} disabled={busy}>
                {t("common.cancel")}
              </button>
              <button
                type="button"
                className={`button button--large ${confirm.kind === "in" ? "button--go" : "button--primary"}`}
                onClick={() => void submit()}
                disabled={busy}
                data-autofocus
              >
                {confirm.kind === "in" ? t("confirm.inYes") : t("confirm.outYes")}
              </button>
            </>
          }
        >
          <p className="confirm__question">
            {confirm.kind === "in" ? t("confirm.inBody", { name: confirm.name }) : t("confirm.outBody", { name: confirm.name })}
          </p>
          <p className="confirm__note">{t("confirm.noUndo")}</p>
          {error && <FormMessage tone="error">{error}</FormMessage>}
        </Dialog>
      )}
    </>
  );
}

/** One section of names, each with its own time and «Σήμανση». */
function Lines({
  title,
  lines,
  kind,
  onMark,
}: {
  title: string;
  lines: ReminderLine[];
  kind: "in" | "out";
  onMark: (line: ReminderLine) => void;
}) {
  const { t } = useTranslation();
  if (lines.length === 0) return null;
  return (
    <section className={`alarm__list alarm__list--${kind}`} aria-label={title}>
      <h2 className="alarm__list-title">{title}</h2>
      <ul className="reminder__lines">
        {lines.map((line) => (
          <li key={`${line.sourceBlockId}-${line.businessDate}-${line.kind}`} className="reminder__line">
            <span className="alarm__name-time">{line.at}</span>
            <span className="reminder__who">
              <span className="reminder__name">{line.name}</span>
              {line.noArrivalMark && <span className="reminder__noarrival">{t("reminder.noArrival")}</span>}
            </span>
            <button
              type="button"
              className={`button reminder__mark ${kind === "in" ? "button--go" : "button--primary"}`}
              onClick={() => onMark(line)}
            >
              {t("reminder.mark")}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Platform } from "../api";
import { Field, FormMessage } from "../components/Field";
import { errorMessage } from "../errors";
import { useAdmin } from "./admin";

const OFFSET_MIN = -60;
const OFFSET_MAX = 30;
const ROLLOVERS = ["00:00", "01:00", "02:00", "03:00", "04:00", "05:00", "06:00", "07:00", "08:00"];
/** «Λείπει σήμανση» choices in minutes; 0 = off (SPEC §4.4). */
const REMINDERS = [0, 5, 10, 15, 20, 30, 45, 60];

/** Every 5 minutes in range, plus the current value if it is in between. */
function offsetChoices(current: number): number[] {
  const values = new Set<number>([current]);
  for (let m = OFFSET_MIN; m <= OFFSET_MAX; m += 5) values.add(m);
  return [...values].sort((a, b) => a - b);
}

/**
 * Settings → offsets, rollover, the «Λείπει σήμανση» reminder, autostart
 * (SPEC §4.4). The quit code is under Codes.
 * Autostart is the shop PC's setting: Android doesn't show it and saves the
 * server's latest value, so a change made on the PC stays.
 */
export function AlarmsPanel({ platform }: { platform: Platform }) {
  const { t } = useTranslation();
  const { view, run } = useAdmin();
  const s = view.settings;
  const [checkin, setCheckin] = useState(s.checkinOffsetMin);
  const [checkout, setCheckout] = useState(s.checkoutOffsetMin);
  const [rollover, setRollover] = useState(s.rollover);
  const [autostart, setAutostart] = useState(s.autostart);
  const [reminder, setReminder] = useState(s.unmarkedReminderMin);
  const reminders = REMINDERS.includes(s.unmarkedReminderMin)
    ? REMINDERS
    : [...REMINDERS, s.unmarkedReminderMin].sort((a, b) => a - b);
  const showAutostart = platform !== "android";
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);

  const rollovers = ROLLOVERS.includes(s.rollover) ? ROLLOVERS : [...ROLLOVERS, s.rollover].sort();

  const offsetLabel = (m: number) =>
    m === 0 ? t("alarms.onTime") : m < 0 ? t("alarms.before", { count: -m }) : t("alarms.after", { count: m });

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setMessage(null);
    try {
      await run(() =>
        api.settingsSave({
          checkinOffsetMin: checkin,
          checkoutOffsetMin: checkout,
          rollover,
          autostart: showAutostart ? autostart : view.settings.autostart,
          quitCode: null,
          unmarkedReminderMin: reminder,
        }),
      );
      setMessage({ tone: "ok", text: t("alarms.saved") });
    } catch (e) {
      setMessage({ tone: "error", text: errorMessage(t, e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="panel">
      <div className="panel__head">
        <h2 className="panel__title">{t("alarms.title")}</h2>
      </div>
      <form className="form" onSubmit={(e) => void save(e)} onChange={() => setMessage(null)}>
        <div className="form__row">
          <Field label={t("alarms.checkinOffset")}>
            <select className="input" value={checkin} onChange={(e) => setCheckin(Number(e.target.value))} disabled={busy}>
              {offsetChoices(s.checkinOffsetMin).map((m) => (
                <option key={m} value={m}>
                  {offsetLabel(m)}
                </option>
              ))}
            </select>
          </Field>
          <Field label={t("alarms.checkoutOffset")}>
            <select className="input" value={checkout} onChange={(e) => setCheckout(Number(e.target.value))} disabled={busy}>
              {offsetChoices(s.checkoutOffsetMin).map((m) => (
                <option key={m} value={m}>
                  {offsetLabel(m)}
                </option>
              ))}
            </select>
          </Field>
        </div>
        <Field label={t("alarms.rollover")} hint={t("alarms.rolloverHint")}>
          <select className="input input--narrow" value={rollover} onChange={(e) => setRollover(e.target.value)} disabled={busy}>
            {rollovers.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </select>
        </Field>
        <Field label={t("alarms.reminder")} hint={t("alarms.reminderHint")}>
          <select className="input" value={reminder} onChange={(e) => setReminder(Number(e.target.value))} disabled={busy}>
            {reminders.map((m) => (
              <option key={m} value={m}>
                {m === 0 ? t("alarms.reminderOff") : t("alarms.reminderAfter", { count: m })}
              </option>
            ))}
          </select>
        </Field>
        {showAutostart && (
          <label className="checkbox">
            <input type="checkbox" checked={autostart} onChange={(e) => setAutostart(e.target.checked)} disabled={busy} />
            {t("alarms.autostart")}
          </label>
        )}
        {message && <FormMessage tone={message.tone}>{message.text}</FormMessage>}
        <div className="form__actions">
          <button type="submit" className="button button--primary" disabled={busy}>
            {busy ? t("common.saving") : t("common.save")}
          </button>
        </div>
      </form>
    </div>
  );
}

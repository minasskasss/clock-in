import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { Field, FormMessage } from "../components/Field";
import { errorMessage, invalidField } from "../errors";
import { useAdmin } from "./admin";

const OFFSET_MIN = -60;
const OFFSET_MAX = 30;
const ROLLOVERS = ["00:00", "01:00", "02:00", "03:00", "04:00", "05:00", "06:00", "07:00", "08:00"];
const QUIT_CODE = /^[0-9]{4}$/;

/** Every 5 minutes in range, plus the current value if it is in between. */
function offsetChoices(current: number): number[] {
  const values = new Set<number>([current]);
  for (let m = OFFSET_MIN; m <= OFFSET_MAX; m += 5) values.add(m);
  return [...values].sort((a, b) => a - b);
}

/** Settings → offsets, rollover, autostart, quit code (SPEC §4.4). */
export function AlarmsPanel() {
  const { t } = useTranslation();
  const { view, run } = useAdmin();
  const s = view.settings;
  const [checkin, setCheckin] = useState(s.checkinOffsetMin);
  const [checkout, setCheckout] = useState(s.checkoutOffsetMin);
  const [rollover, setRollover] = useState(s.rollover);
  const [autostart, setAutostart] = useState(s.autostart);
  const [quitCode, setQuitCode] = useState("");
  const [quitCode2, setQuitCode2] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);

  const quitCodeBad = quitCode !== "" && !QUIT_CODE.test(quitCode);
  const quitCodeMismatch = quitCode2 !== quitCode;
  const rollovers = ROLLOVERS.includes(s.rollover) ? ROLLOVERS : [...ROLLOVERS, s.rollover].sort();

  const offsetLabel = (m: number) =>
    m === 0 ? t("alarms.onTime") : m < 0 ? t("alarms.before", { count: -m }) : t("alarms.after", { count: m });

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (quitCodeBad || quitCodeMismatch) return;
    setBusy(true);
    setMessage(null);
    try {
      await run(() =>
        api.settingsSave({
          checkinOffsetMin: checkin,
          checkoutOffsetMin: checkout,
          rollover,
          autostart,
          quitCode: quitCode === "" ? null : quitCode,
        }),
      );
      setQuitCode("");
      setQuitCode2("");
      setMessage({ tone: "ok", text: t("alarms.saved") });
    } catch (e) {
      setMessage({ tone: "error", text: invalidField(e) === "quit_code" ? t("errors.quit_code") : errorMessage(t, e) });
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
        <label className="checkbox">
          <input type="checkbox" checked={autostart} onChange={(e) => setAutostart(e.target.checked)} disabled={busy} />
          {t("alarms.autostart")}
        </label>
        <div className="form__row">
          <Field label={t("alarms.quitCode")} hint={t("alarms.quitCodeHint")} error={quitCodeBad ? t("setup.quitCodeFormat") : null}>
            <input
              className="input input--code"
              inputMode="numeric"
              maxLength={4}
              autoComplete="off"
              value={quitCode}
              onChange={(e) => setQuitCode(e.target.value.replace(/\D/g, ""))}
              disabled={busy}
            />
          </Field>
          <Field
            label={t("alarms.quitCodeRepeat")}
            error={quitCode2 !== "" && quitCodeMismatch ? t("setup.quitCodeMismatch") : null}
          >
            <input
              className="input input--code"
              inputMode="numeric"
              maxLength={4}
              autoComplete="off"
              value={quitCode2}
              onChange={(e) => setQuitCode2(e.target.value.replace(/\D/g, ""))}
              disabled={busy}
            />
          </Field>
        </div>
        {message && <FormMessage tone={message.tone}>{message.text}</FormMessage>}
        <div className="form__actions">
          <button type="submit" className="button button--primary" disabled={busy || quitCodeBad || quitCodeMismatch}>
            {busy ? t("common.saving") : t("common.save")}
          </button>
        </div>
      </form>
    </div>
  );
}

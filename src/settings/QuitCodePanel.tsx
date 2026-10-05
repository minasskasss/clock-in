import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { Field, FormMessage } from "../components/Field";
import { errorMessage, invalidField } from "../errors";
import { useAdmin } from "./admin";

const QUIT_CODE = /^[0-9]{4}$/;

/** Settings → Codes → Change quit code (SPEC §4.4). The other settings are sent unchanged. */
export function QuitCodePanel() {
  const { t } = useTranslation();
  const { view, run } = useAdmin();
  const [code, setCode] = useState("");
  const [code2, setCode2] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);

  const bad = code !== "" && !QUIT_CODE.test(code);
  const mismatch = code2 !== "" && code2 !== code;
  const ready = QUIT_CODE.test(code) && code2 === code;

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!ready || busy) return;
    const s = view.settings;
    setBusy(true);
    setMessage(null);
    try {
      await run(() =>
        api.settingsSave({
          checkinOffsetMin: s.checkinOffsetMin,
          checkoutOffsetMin: s.checkoutOffsetMin,
          rollover: s.rollover,
          autostart: s.autostart,
          quitCode: code,
        }),
      );
      setCode("");
      setCode2("");
      setMessage({ tone: "ok", text: t("quitCode.done") });
    } catch (e) {
      setMessage({ tone: "error", text: invalidField(e) === "quit_code" ? t("errors.quit_code") : errorMessage(t, e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="panel">
      <div className="panel__head">
        <h2 className="panel__title">{t("quitCode.title")}</h2>
      </div>
      <p className="panel__intro">{t("quitCode.intro")}</p>
      <form className="form" onSubmit={(e) => void save(e)} onChange={() => setMessage(null)}>
        <div className="form__row">
          <Field label={t("quitCode.new")} error={bad ? t("setup.quitCodeFormat") : null}>
            <input
              className="input input--code"
              inputMode="numeric"
              maxLength={4}
              autoComplete="off"
              value={code}
              onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))}
              disabled={busy}
            />
          </Field>
          <Field label={t("quitCode.repeat")} error={mismatch ? t("setup.quitCodeMismatch") : null}>
            <input
              className="input input--code"
              inputMode="numeric"
              maxLength={4}
              autoComplete="off"
              value={code2}
              onChange={(e) => setCode2(e.target.value.replace(/\D/g, ""))}
              disabled={busy}
            />
          </Field>
        </div>
        {message && <FormMessage tone={message.tone}>{message.text}</FormMessage>}
        <div className="form__actions">
          <button type="submit" className="button button--primary" disabled={!ready || busy}>
            {busy ? t("common.saving") : t("quitCode.submit")}
          </button>
        </div>
      </form>
    </div>
  );
}

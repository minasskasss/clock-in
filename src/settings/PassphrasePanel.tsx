import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api, toCmdError } from "../api";
import { Dialog } from "../components/Dialog";
import { Field, FormMessage } from "../components/Field";
import { PassphraseInput } from "../components/PassphraseInput";
import { errorMessage } from "../errors";
import { formatCountdown } from "../format";
import { useNewPassphraseCheck } from "../usePassphraseCheck";
import { useAdmin } from "./admin";

interface PassphrasePanelProps {
  lockoutRemainingS: number;
  /** After a change: Settings lock (SPEC §4.6). */
  onChanged: () => void;
}

/** Settings → Change passphrase, with Generate (SPEC §4.6). */
export function PassphrasePanel({ lockoutRemainingS, onChanged }: PassphrasePanelProps) {
  const { t } = useTranslation();
  const { run } = useAdmin();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [next2, setNext2] = useState("");
  const [generated, setGenerated] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const check = useNewPassphraseCheck(next);
  const locked = lockoutRemainingS > 0;
  const mismatch = next2 !== "" && next2.trim() !== next.trim();
  const ready = current.trim() !== "" && check === "ok" && next2.trim() === next.trim() && !locked;

  const generate = async () => {
    setError(null);
    try {
      const words = await run(() => api.generatePassphrase());
      setNext(words);
      setNext2(words);
      setGenerated(true);
    } catch (e) {
      setError(errorMessage(t, e));
    }
  };

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!ready || busy) return;
    setBusy(true);
    setError(null);
    try {
      await run(() => api.changePassphrase(current, next));
      setCurrent("");
      setNext("");
      setNext2("");
      setDone(true);
    } catch (e) {
      const err = toCmdError(e);
      setCurrent("");
      setError(err.kind === "rejected" && err.code === "locked" ? null : errorMessage(t, err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="panel">
      <div className="panel__head">
        <h2 className="panel__title">{t("changePassphrase.title")}</h2>
      </div>
      <p className="panel__intro">{t("changePassphrase.intro")}</p>
      <form className="form" onSubmit={(e) => void submit(e)}>
        <Field label={t("changePassphrase.current")}>
          <PassphraseInput value={current} onChange={setCurrent} disabled={busy || locked} />
        </Field>
        <Field
          label={t("changePassphrase.new")}
          error={check !== "ok" ? check : null}
          hint={generated ? <strong>{t("changePassphrase.generatedNote")}</strong> : undefined}
        >
          <PassphraseInput
            value={next}
            onChange={(v) => {
              setNext(v);
              setGenerated(false);
            }}
            revealed={generated}
            disabled={busy}
          />
        </Field>
        <div>
          <button type="button" className="button" onClick={() => void generate()} disabled={busy}>
            {t("changePassphrase.generate")}
          </button>
        </div>
        <Field label={t("changePassphrase.repeat")} error={mismatch ? t("changePassphrase.mismatch") : null}>
          <PassphraseInput value={next2} onChange={setNext2} revealed={generated} disabled={busy} />
        </Field>
        {locked && <FormMessage tone="error">{t("lockout.locked", { time: formatCountdown(lockoutRemainingS) })}</FormMessage>}
        {!locked && error && <FormMessage tone="error">{error}</FormMessage>}
        <div className="form__actions">
          <button type="submit" className="button button--primary" disabled={!ready || busy}>
            {busy ? t("common.saving") : t("changePassphrase.submit")}
          </button>
        </div>
      </form>
      {done && (
        <Dialog
          title={t("changePassphrase.doneTitle")}
          actions={
            <button type="button" className="button button--primary" onClick={onChanged} data-autofocus>
              {t("common.ok")}
            </button>
          }
        >
          <p>{t("changePassphrase.doneBody")}</p>
        </Dialog>
      )}
    </div>
  );
}

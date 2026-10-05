import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api, toCmdError } from "../api";
import { errorMessage } from "../errors";
import { formatCountdown } from "../format";
import { Dialog } from "./Dialog";
import { Field, FormMessage } from "./Field";
import { PassphraseInput } from "./PassphraseInput";

interface UnlockProps {
  /** `reauth`: Settings are open but the server session ran out. */
  mode: "open" | "reauth";
  /** From the polled app state; counts down while the lockout lasts. */
  lockoutRemainingS: number;
  onUnlocked: () => void;
  onCancel: () => void;
}

/** The passphrase prompt in front of Settings (SPEC §2, §4.6). */
export function Unlock({ mode, lockoutRemainingS, onUnlocked, onCancel }: UnlockProps) {
  const { t } = useTranslation();
  const [passphrase, setPassphrase] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const locked = lockoutRemainingS > 0;

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (busy || locked || passphrase.trim() === "") return;
    setBusy(true);
    setError(null);
    try {
      await api.adminLogin(passphrase);
      setPassphrase("");
      onUnlocked();
    } catch (e) {
      const err = toCmdError(e);
      setPassphrase("");
      // A lockout shows as the live countdown below instead.
      setError(err.kind === "rejected" && err.code === "locked" ? null : errorMessage(t, err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog title={t("unlock.title")} onClose={busy ? undefined : onCancel}>
      <form onSubmit={(e) => void submit(e)} className="form">
        <p>{mode === "reauth" ? t("unlock.reauth") : t("unlock.intro")}</p>
        <Field label={t("unlock.passphrase")}>
          <PassphraseInput value={passphrase} onChange={setPassphrase} disabled={busy || locked} data-autofocus />
        </Field>
        {locked && <FormMessage tone="error">{t("lockout.locked", { time: formatCountdown(lockoutRemainingS) })}</FormMessage>}
        {!locked && error && <FormMessage tone="error">{error}</FormMessage>}
        <div className="dialog__actions">
          <button type="button" className="button button--quiet" onClick={onCancel} disabled={busy}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="button button--primary" disabled={busy || locked || passphrase.trim() === ""}>
            {busy ? t("unlock.submitting") : t("unlock.submit")}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api, toCmdError } from "../api";
import { errorMessage } from "../errors";
import { formatCountdown } from "../format";
import { Field, FormMessage } from "./Field";
import { PassphraseInput } from "./PassphraseInput";
import { useNewPassphraseCheck } from "../usePassphraseCheck";
import "./FirstRun.css";

interface FirstRunProps {
  defaultDeviceName: string;
  lockoutRemainingS: number;
  onDone: () => void;
}

/** The screens of an unpaired device: choose, then set up or pair. */
export function FirstRun({ defaultDeviceName, lockoutRemainingS, onDone }: FirstRunProps) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<"choose" | "first" | "pair">("choose");

  if (mode === "first") {
    return <SetupFirst defaultDeviceName={defaultDeviceName} onBack={() => setMode("choose")} onDone={onDone} />;
  }
  if (mode === "pair") {
    return (
      <PairDevice
        defaultDeviceName={defaultDeviceName}
        lockoutRemainingS={lockoutRemainingS}
        onBack={() => setMode("choose")}
        onDone={onDone}
      />
    );
  }
  return (
    <section className="first-run card" aria-labelledby="welcome-title">
      <h1 id="welcome-title" className="first-run__title">
        {t("welcome.title")}
      </h1>
      <p className="first-run__intro">{t("welcome.intro")}</p>
      <div className="choices">
        <button type="button" className="choice" onClick={() => setMode("pair")}>
          <span className="choice__title">{t("welcome.pair")}</span>
          <span className="choice__hint">{t("welcome.pairHint")}</span>
        </button>
        <button type="button" className="choice" onClick={() => setMode("first")}>
          <span className="choice__title">{t("welcome.first")}</span>
          <span className="choice__hint">{t("welcome.firstHint")}</span>
        </button>
      </div>
    </section>
  );
}

function BackButton({ onBack, disabled }: { onBack: () => void; disabled?: boolean }) {
  const { t } = useTranslation();
  return (
    <button type="button" className="button button--quiet first-run__back" onClick={onBack} disabled={disabled}>
      ← {t("common.back")}
    </button>
  );
}

const QUIT_CODE = /^[0-9]{4}$/;

function SetupFirst({ defaultDeviceName, onBack, onDone }: { defaultDeviceName: string; onBack: () => void; onDone: () => void }) {
  const { t } = useTranslation();
  const [passphrase, setPassphrase] = useState("");
  const [passphrase2, setPassphrase2] = useState("");
  const [quitCode, setQuitCode] = useState("");
  const [quitCode2, setQuitCode2] = useState("");
  const [deviceName, setDeviceName] = useState(defaultDeviceName);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const check = useNewPassphraseCheck(passphrase);

  const passphraseMismatch = passphrase2 !== "" && passphrase2.trim() !== passphrase.trim();
  const quitCodeBad = quitCode !== "" && !QUIT_CODE.test(quitCode);
  const quitCodeMismatch = quitCode2 !== "" && quitCode2 !== quitCode;
  const ready =
    check === "ok" &&
    passphrase2.trim() === passphrase.trim() &&
    QUIT_CODE.test(quitCode) &&
    quitCode2 === quitCode &&
    deviceName.trim() !== "";

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!ready || busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.initialize(passphrase, quitCode, deviceName);
      onDone();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="first-run card" aria-labelledby="setup-title">
      <BackButton onBack={onBack} disabled={busy} />
      <h1 id="setup-title" className="first-run__title">
        {t("setup.title")}
      </h1>
      <form className="form" onSubmit={(e) => void submit(e)}>
        <fieldset className="form__step">
          <legend>{t("setup.stepPassphrase")}</legend>
          <p className="form__intro">{t("setup.passphraseIntro")}</p>
          <Field
            label={t("setup.passphrase")}
            error={check !== "ok" ? check : null}
            hint={check === "ok" ? <span className="ok">✓ {t("setup.passphraseOk")}</span> : undefined}
          >
            <PassphraseInput value={passphrase} onChange={setPassphrase} disabled={busy} data-autofocus />
          </Field>
          <Field label={t("setup.passphraseRepeat")} error={passphraseMismatch ? t("setup.passphraseMismatch") : null}>
            <PassphraseInput value={passphrase2} onChange={setPassphrase2} disabled={busy} />
          </Field>
        </fieldset>
        <fieldset className="form__step">
          <legend>{t("setup.stepQuitCode")}</legend>
          <p className="form__intro">{t("setup.quitCodeIntro")}</p>
          <div className="form__row">
            <Field label={t("setup.quitCode")} error={quitCodeBad ? t("setup.quitCodeFormat") : null}>
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
            <Field label={t("setup.quitCodeRepeat")} error={quitCodeMismatch ? t("setup.quitCodeMismatch") : null}>
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
        </fieldset>
        <fieldset className="form__step">
          <legend>{t("setup.stepDevice")}</legend>
          <Field label={t("setup.deviceName")} hint={t("setup.deviceIntro")}>
            <input className="input" maxLength={60} value={deviceName} onChange={(e) => setDeviceName(e.target.value)} disabled={busy} />
          </Field>
        </fieldset>
        {error && <FormMessage tone="error">{error}</FormMessage>}
        <button type="submit" className="button button--primary button--large" disabled={!ready || busy}>
          {busy ? t("setup.submitting") : t("setup.submit")}
        </button>
      </form>
    </section>
  );
}

function PairDevice({
  defaultDeviceName,
  lockoutRemainingS,
  onBack,
  onDone,
}: {
  defaultDeviceName: string;
  lockoutRemainingS: number;
  onBack: () => void;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const [passphrase, setPassphrase] = useState("");
  const [deviceName, setDeviceName] = useState(defaultDeviceName);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const locked = lockoutRemainingS > 0;

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (busy || locked || passphrase.trim() === "" || deviceName.trim() === "") return;
    setBusy(true);
    setError(null);
    try {
      await api.pair(passphrase, deviceName);
      onDone();
    } catch (e) {
      const err = toCmdError(e);
      setPassphrase("");
      setError(err.kind === "rejected" && err.code === "locked" ? null : errorMessage(t, err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="first-run card" aria-labelledby="pair-title">
      <BackButton onBack={onBack} disabled={busy} />
      <h1 id="pair-title" className="first-run__title">
        {t("pair.title")}
      </h1>
      <form className="form" onSubmit={(e) => void submit(e)}>
        <p className="form__intro">{t("pair.intro")}</p>
        <Field label={t("pair.passphrase")}>
          <PassphraseInput value={passphrase} onChange={setPassphrase} disabled={busy || locked} data-autofocus />
        </Field>
        <Field label={t("pair.deviceName")} hint={t("setup.deviceIntro")}>
          <input className="input" maxLength={60} value={deviceName} onChange={(e) => setDeviceName(e.target.value)} disabled={busy} />
        </Field>
        {locked && <FormMessage tone="error">{t("lockout.locked", { time: formatCountdown(lockoutRemainingS) })}</FormMessage>}
        {!locked && error && <FormMessage tone="error">{error}</FormMessage>}
        <button
          type="submit"
          className="button button--primary button--large"
          disabled={busy || locked || passphrase.trim() === "" || deviceName.trim() === ""}
        >
          {busy ? t("pair.submitting") : t("pair.submit")}
        </button>
      </form>
    </section>
  );
}

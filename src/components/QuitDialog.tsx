import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { Dialog } from "./Dialog";
import { FormMessage } from "./Field";
import "./QuitDialog.css";

const CODE_LENGTH = 4;
const KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];

interface QuitDialogProps {
  /** False before a quit code is known (unpaired): only a confirmation. */
  codeSet: boolean;
  onCancel: () => void;
}

/**
 * Tray → Quit (SPEC §8.1): a 4-digit keypad. Rust checks the code on this
 * device (it works offline) and exits only if it is right; a wrong code
 * keeps the app running. No lockout: the code only prevents closing by
 * accident. Digits can also be typed on the keyboard.
 */
export function QuitDialog({ codeSet, onCancel }: QuitDialogProps) {
  const { t } = useTranslation();
  const [digits, setDigitsState] = useState("");
  const [wrong, setWrong] = useState(false);
  const [busy, setBusy] = useState(false);
  // Refs, so two quick key presses between renders both count.
  const busyRef = useRef(false);
  const digitsRef = useRef("");
  const setDigits = (next: string) => {
    digitsRef.current = next;
    setDigitsState(next);
  };

  const submit = async (code: string) => {
    busyRef.current = true;
    setBusy(true);
    try {
      // On success the app exits and nothing more happens here.
      await api.quit(code);
    } catch {
      setWrong(true);
      setDigits("");
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const press = (digit: string) => {
    if (busyRef.current || digitsRef.current.length >= CODE_LENGTH) return;
    setWrong(false);
    const next = digitsRef.current + digit;
    setDigits(next);
    if (next.length === CODE_LENGTH) void submit(next);
  };

  const backspace = () => {
    if (busyRef.current) return;
    setDigits(digitsRef.current.slice(0, -1));
  };

  // Typing the digits works too.
  const keys = useRef({ press, backspace });
  useEffect(() => {
    keys.current = { press, backspace };
  });
  useEffect(() => {
    if (!codeSet) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (/^[0-9]$/.test(event.key)) {
        event.preventDefault();
        keys.current.press(event.key);
      } else if (event.key === "Backspace") {
        event.preventDefault();
        keys.current.backspace();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [codeSet]);

  if (!codeSet) {
    return (
      <Dialog
        title={t("quit.title")}
        onClose={onCancel}
        actions={
          <>
            <button type="button" className="button button--quiet button--large" onClick={onCancel} data-autofocus>
              {t("common.cancel")}
            </button>
            <button type="button" className="button button--danger button--large" onClick={() => void submit("")} disabled={busy}>
              {t("quit.submit")}
            </button>
          </>
        }
      >
        <p>{t("quit.noCode")}</p>
      </Dialog>
    );
  }

  return (
    <Dialog
      title={t("quit.title")}
      onClose={onCancel}
      actions={
        <button type="button" className="button button--quiet button--large" onClick={onCancel}>
          {t("common.cancel")}
        </button>
      }
    >
      <p>{t("quit.intro")}</p>
      <div className="keypad__display" role="group" aria-label={t("quit.code")} data-filled={digits.length}>
        {Array.from({ length: CODE_LENGTH }, (_, i) => (
          <span key={i} className={`keypad__slot${i < digits.length ? " keypad__slot--filled" : ""}`} aria-hidden="true" />
        ))}
      </div>
      {wrong && <FormMessage tone="error">{t("quit.wrong")}</FormMessage>}
      <div className="keypad">
        {KEYS.map((key) => (
          <button key={key} type="button" className="button keypad__key" onClick={() => press(key)} disabled={busy} data-autofocus={key === "1" ? true : undefined}>
            {key}
          </button>
        ))}
        <button type="button" className="button keypad__key keypad__key--quiet" onClick={backspace} disabled={busy} aria-label={t("quit.backspace")}>
          ⌫
        </button>
        <button type="button" className="button keypad__key" onClick={() => press("0")} disabled={busy}>
          0
        </button>
      </div>
    </Dialog>
  );
}

import { useState, type InputHTMLAttributes } from "react";
import { useTranslation } from "react-i18next";

type PassphraseInputProps = Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "onChange"> & {
  value: string;
  onChange: (value: string) => void;
  /** Start visible (e.g. a freshly generated passphrase to write down). */
  revealed?: boolean;
};

/**
 * A passphrase box, hidden by default with a Show button: five words typed
 * blind are easy to get wrong. Never autocompleted or spell-checked.
 */
export function PassphraseInput({ value, onChange, revealed, ...rest }: PassphraseInputProps) {
  const { t } = useTranslation();
  const [shown, setShown] = useState(false);
  const visible = shown || !!revealed;
  return (
    <div className="passphrase-input">
      <input
        {...rest}
        className="input passphrase-input__box"
        type={visible ? "text" : "password"}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        autoComplete="off"
        autoCapitalize="none"
        autoCorrect="off"
        spellCheck={false}
      />
      <button
        type="button"
        className="button button--quiet passphrase-input__toggle"
        aria-pressed={visible}
        onClick={() => setShown((v) => !v)}
      >
        {visible ? t("passphrase.hide") : t("passphrase.show")}
      </button>
    </div>
  );
}

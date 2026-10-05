import { useState, type InputHTMLAttributes } from "react";
import { useTranslation } from "react-i18next";

type PassphraseInputProps = Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "onChange"> & {
  value: string;
  onChange: (value: string) => void;
  /**
   * Visible or hidden, when the parent decides (e.g. shown after Generate so
   * it can be written down). The button always flips it through
   * `onShownChange`. Without these the box keeps its own state.
   */
  shown?: boolean;
  onShownChange?: (shown: boolean) => void;
};

/**
 * A passphrase box, hidden by default with a Show button: five words typed
 * blind are easy to get wrong. Never autocompleted or spell-checked.
 */
export function PassphraseInput({ value, onChange, shown, onShownChange, ...rest }: PassphraseInputProps) {
  const { t } = useTranslation();
  const [ownShown, setOwnShown] = useState(false);
  const visible = shown ?? ownShown;
  const toggle = () => {
    setOwnShown(!visible);
    onShownChange?.(!visible);
  };
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
        onClick={toggle}
      >
        {visible ? t("passphrase.hide") : t("passphrase.show")}
      </button>
    </div>
  );
}

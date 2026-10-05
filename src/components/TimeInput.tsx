import type { InputHTMLAttributes } from "react";
import { useTranslation } from "react-i18next";
import { tidyTime } from "../format";

type TimeInputProps = Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "onChange" | "value"> & {
  value: string;
  onChange: (value: string) => void;
};

/**
 * A 24-hour "HH:MM" text box. A native time picker would follow the OS
 * locale and could show AM/PM; SPEC §4.2 wants 24-hour times everywhere.
 */
export function TimeInput({ value, onChange, onBlur, ...rest }: TimeInputProps) {
  const { t } = useTranslation();
  return (
    <input
      {...rest}
      className="input input--time"
      type="text"
      inputMode="numeric"
      autoComplete="off"
      placeholder={t("common.timePlaceholder")}
      maxLength={5}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      onBlur={(e) => {
        const tidy = tidyTime(e.target.value);
        if (tidy !== e.target.value) onChange(tidy);
        onBlur?.(e);
      }}
    />
  );
}

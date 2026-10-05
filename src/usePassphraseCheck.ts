import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, toCmdError } from "./api";
import { passphraseProblem } from "./errors";

/**
 * Live check of a new passphrase against the EFF list (in Rust): `"ok"`, a
 * problem message, or `null` while empty or not checked yet.
 */
export function useNewPassphraseCheck(passphrase: string): string | null | "ok" {
  const { t } = useTranslation();
  const [result, setResult] = useState<{ input: string; value: string | null | "ok" } | null>(null);
  const empty = passphrase.trim() === "";

  useEffect(() => {
    if (empty) return;
    let current = true;
    api.checkNewPassphrase(passphrase).then(
      () => current && setResult({ input: passphrase, value: "ok" }),
      (e: unknown) => {
        const err = toCmdError(e);
        const value = err.kind === "invalid" ? passphraseProblem(t, err.problem, err.positions) : null;
        if (current) setResult({ input: passphrase, value });
      },
    );
    return () => {
      current = false;
    };
  }, [passphrase, empty, t]);

  return empty || result?.input !== passphrase ? null : result.value;
}

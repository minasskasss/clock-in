import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, toCmdError } from "./api";
import { passphraseProblem } from "./errors";

/**
 * Live check of a new passphrase against the EFF list (in Rust): `"ok"`, a
 * problem message, or `null` while empty or not checked yet. With `current`
 * (the current passphrase typed in the same form), a new one equal to it
 * after normalisation is refused too.
 */
export function useNewPassphraseCheck(passphrase: string, current = ""): string | null | "ok" {
  const { t } = useTranslation();
  const [result, setResult] = useState<{ input: string; current: string; value: string | null | "ok" } | null>(null);
  const empty = passphrase.trim() === "";

  useEffect(() => {
    if (empty) return;
    let live = true;
    api.checkNewPassphrase(passphrase, current.trim() === "" ? undefined : current).then(
      () => live && setResult({ input: passphrase, current, value: "ok" }),
      (e: unknown) => {
        const err = toCmdError(e);
        const value = err.kind === "invalid" ? passphraseProblem(t, err.problem, err.positions) : null;
        if (live) setResult({ input: passphrase, current, value });
      },
    );
    return () => {
      live = false;
    };
  }, [passphrase, current, empty, t]);

  return empty || result?.input !== passphrase || result.current !== current ? null : result.value;
}

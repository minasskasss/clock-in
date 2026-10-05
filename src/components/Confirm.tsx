import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { errorMessage } from "../errors";
import { Dialog } from "./Dialog";
import { FormMessage } from "./Field";

interface ConfirmProps {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  /** Destructive actions get the warning colour. */
  danger?: boolean;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}

/** "Are you sure?" with the action running inside, and its error shown in place. */
export function Confirm({ title, children, confirmLabel, danger, onConfirm, onClose }: ConfirmProps) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await onConfirm();
      onClose();
    } catch (e) {
      setError(errorMessage(t, e));
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={title}
      onClose={busy ? undefined : onClose}
      actions={
        <>
          <button type="button" className="button button--quiet" onClick={onClose} disabled={busy} data-autofocus>
            {t("common.cancel")}
          </button>
          <button
            type="button"
            className={`button ${danger ? "button--danger" : "button--primary"}`}
            onClick={() => void confirm()}
            disabled={busy}
          >
            {busy ? t("common.saving") : confirmLabel}
          </button>
        </>
      }
    >
      {children}
      {error && <FormMessage tone="error">{error}</FormMessage>}
    </Dialog>
  );
}

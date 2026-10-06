import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { api, type PermissionKind, type PermissionStatus } from "../api";
import "./Onboarding.css";

interface OnboardingProps {
  /** Null until Android was first asked. */
  permissions: PermissionStatus | null;
  /** Everything the alarms need is granted. */
  permissionsOk: boolean;
  onDone: () => void;
}

const ITEMS = ["notifications", "exactAlarms", "fullScreen", "battery", "unusedApps"] as const;

/**
 * Android onboarding (SPEC §8.2): every permission the alarms need, with a
 * live ✓ / ✗ and a button to the phone's own screen that fixes it. Plain
 * words, so it can be followed over the phone.
 */
export function Onboarding({ permissions, permissionsOk, onDone }: OnboardingProps) {
  const { t } = useTranslation();
  const open = (kind: PermissionKind) => void api.androidOpenSettings(kind).catch(() => {});
  const oem = permissions?.oem ?? "";
  const oemKey = oem === "samsung" ? "oemSamsung" : "oemOther";

  return (
    <section className="onboarding card" aria-labelledby="onboarding-title">
      <h1 id="onboarding-title" className="onboarding__title">
        {t("onboarding.title")}
      </h1>
      <p className="onboarding__intro">{t("onboarding.intro")}</p>
      {permissions === null ? (
        <p className="onboarding__intro">{t("onboarding.checking")}</p>
      ) : (
        <ol className="checklist">
          {ITEMS.map((kind) => (
            <ChecklistItem
              key={kind}
              ok={permissions[kind]}
              title={t(`onboarding.${kind}`)}
              help={t(`onboarding.${kind}Help`)}
              onFix={() => open(kind)}
            />
          ))}
          {oem !== "" && (
            <ChecklistItem
              ok={permissions.oemDone}
              title={t(`onboarding.${oemKey}`)}
              help={t(`onboarding.${oemKey}Help`)}
              onFix={() => open("oem")}
              extra={
                <button
                  type="button"
                  className="button button--quiet"
                  onClick={() => void api.androidSetOemDone(!permissions.oemDone).catch(() => {})}
                >
                  {permissions.oemDone ? t("onboarding.oemUndo") : t("onboarding.oemDone")}
                </button>
              }
            />
          )}
        </ol>
      )}
      {permissionsOk && permissions !== null && <p className="onboarding__done">{t("onboarding.allDone")}</p>}
      <div className="onboarding__actions">
        <button
          type="button"
          className={`button button--large ${permissionsOk ? "button--go" : "button--quiet"}`}
          onClick={onDone}
        >
          {permissionsOk ? t("onboarding.continue") : t("onboarding.later")}
        </button>
      </div>
    </section>
  );
}

function ChecklistItem({
  ok,
  title,
  help,
  onFix,
  extra,
}: {
  ok: boolean;
  title: string;
  help: string;
  onFix: () => void;
  extra?: ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <li className={`checklist__item ${ok ? "checklist__item--ok" : "checklist__item--missing"}`}>
      <span className="checklist__mark" aria-label={ok ? t("onboarding.granted") : t("onboarding.missing")}>
        {ok ? "✓" : "✗"}
      </span>
      <div className="checklist__body">
        <p className="checklist__title">{title}</p>
        {!ok && <p className="checklist__help">{help}</p>}
        <div className="checklist__actions">
          {!ok && (
            <button type="button" className="button button--primary" onClick={onFix}>
              {t("onboarding.fix")}
            </button>
          )}
          {extra}
        </div>
      </div>
    </li>
  );
}

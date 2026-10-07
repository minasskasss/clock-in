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

/** Xiaomi / Redmi / POCO (MIUI, HyperOS): steps with their own screens (dontkillmyapp.com). */
const XIAOMI_STEPS = ["xiaomiAutostart", "xiaomiPermissions", "xiaomiBattery", "xiaomiRecents"] as const;
const XIAOMI_LINKS: Partial<Record<(typeof XIAOMI_STEPS)[number], PermissionKind>> = {
  xiaomiAutostart: "xiaomiAutostart",
  xiaomiPermissions: "xiaomiPermissions",
  xiaomiBattery: "xiaomiBattery",
};

/**
 * Android onboarding (SPEC §8.2): every permission the alarms need, with a
 * live ✓ / ✗ and a button to the phone's own screen that fixes it. Plain
 * words, so it can be followed over the phone.
 */
export function Onboarding({ permissions, permissionsOk, onDone }: OnboardingProps) {
  const { t } = useTranslation();
  const open = (kind: PermissionKind) => void api.androidOpenSettings(kind).catch(() => {});
  const oem = permissions?.oem ?? "";
  const oemKey = oem === "samsung" ? "oemSamsung" : oem === "xiaomi" ? "oemXiaomi" : "oemOther";

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
              onFix={oem === "xiaomi" ? undefined : () => open("oem")}
              steps={
                oem === "xiaomi" && (
                  <ol className="checklist__steps">
                    {XIAOMI_STEPS.map((step) => {
                      const link = XIAOMI_LINKS[step];
                      return (
                        <li key={step}>
                          <span>{t(`onboarding.${step}`)}</span>
                          {link && (
                            <button type="button" className="button" onClick={() => open(link)}>
                              {t("onboarding.open")}
                            </button>
                          )}
                        </li>
                      );
                    })}
                  </ol>
                )
              }
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
  steps,
  extra,
}: {
  ok: boolean;
  title: string;
  help: string;
  /** Opens the one screen that fixes it; none when `steps` has their own buttons. */
  onFix?: () => void;
  steps?: ReactNode;
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
        {!ok && steps}
        <div className="checklist__actions">
          {!ok && onFix && (
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

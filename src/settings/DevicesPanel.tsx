import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type DeviceView } from "../api";
import { Confirm } from "../components/Confirm";
import { formatStamp } from "../format";
import { useAdmin } from "./admin";

/** Settings → Devices: the paired devices; revoke one (SPEC §2). */
export function DevicesPanel() {
  const { t } = useTranslation();
  const { view, run } = useAdmin();
  const [revoking, setRevoking] = useState<DeviceView | null>(null);

  return (
    <div className="panel">
      <div className="panel__head">
        <h2 className="panel__title">{t("devices.title")}</h2>
      </div>
      <p className="panel__intro">{t("devices.intro")}</p>
      <ul className="list">
        {view.devices.map((d) => (
          <li key={d.id} className="list__item">
            <div className="list__main">
              <p className="list__title">
                {d.name} {d.thisDevice && <span className="tag">{t("devices.thisDevice")}</span>}
              </p>
              <p className="list__detail">
                {t(`devices.${d.platform}`)} · {t("devices.pairedAt", { when: formatStamp(d.pairedAt, null) })} ·{" "}
                {d.lastSeen ? t("devices.lastSeen", { when: formatStamp(d.lastSeen, view.today) }) : t("devices.never")}
              </p>
            </div>
            <div className="list__actions">
              <button type="button" className="button button--quiet button--danger-text" onClick={() => setRevoking(d)}>
                {t("devices.revoke")}
              </button>
            </div>
          </li>
        ))}
      </ul>
      {revoking && (
        <Confirm
          title={t("devices.revokeTitle")}
          confirmLabel={t("devices.revokeYes")}
          danger
          onConfirm={() => run(() => api.deviceRevoke(revoking.id))}
          onClose={() => setRevoking(null)}
        >
          <p>{t("devices.revokeBody", { name: revoking.name })}</p>
          {revoking.thisDevice && <p className="warning">{t("devices.revokeSelf")}</p>}
        </Confirm>
      )}
    </div>
  );
}

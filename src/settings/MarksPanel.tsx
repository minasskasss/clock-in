import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type MarkView } from "../api";
import { Confirm } from "../components/Confirm";
import { formatHours } from "../format";
import { useAdmin } from "./admin";

/** Settings → Today's marks (SPEC §4.5): remove a mistaken mark. */
export function MarksPanel() {
  const { t } = useTranslation();
  const { view, run } = useAdmin();
  const [removing, setRemoving] = useState<MarkView | null>(null);

  return (
    <div className="panel">
      <div className="panel__head">
        <h2 className="panel__title">{t("marks.title")}</h2>
      </div>
      <p className="panel__intro">{t("marks.intro")}</p>
      {view.marks.length === 0 ? (
        <p className="panel__empty">{t("marks.empty")}</p>
      ) : (
        <ul className="list">
          {view.marks.map((m) => (
            <li key={m.id} className="list__item">
              <div className="list__main">
                <p className="list__title">
                  {m.firstName} {m.lastName}
                </p>
                <p className="list__detail">
                  <span className={`tag tag--${m.kind}`}>{t(`marks.${m.kind}`)}</span> {t("marks.at", { time: m.markedAt.time })}
                  {m.block && <> · {t("marks.shift", { hours: formatHours(t, m.block) })}</>}
                </p>
              </div>
              <div className="list__actions">
                <button type="button" className="button button--quiet button--danger-text" onClick={() => setRemoving(m)}>
                  {t("common.remove")}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {removing && (
        <Confirm
          title={t("marks.removeTitle")}
          confirmLabel={t("marks.removeYes")}
          danger
          onConfirm={() => run(() => api.markVoid(removing.id))}
          onClose={() => setRemoving(null)}
        >
          <p>
            {t("marks.removeBody", {
              kind: t(`marks.${removing.kind}`),
              name: `${removing.firstName} ${removing.lastName}`,
            })}
          </p>
        </Confirm>
      )}
    </div>
  );
}

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type OverrideKind, type OverrideReport, type OverrideView, type RangeDraft } from "../api";
import { Confirm } from "../components/Confirm";
import { Dialog } from "../components/Dialog";
import { Field, FormMessage } from "../components/Field";
import { errorMessage } from "../errors";
import { formatHours, formatShortDate } from "../format";
import { useAdmin } from "./admin";
import { BlockList } from "./BlockList";
import { newUid, type EditBlock } from "./blocks";

export function OverridesPanel() {
  const { t } = useTranslation();
  const { view, run } = useAdmin();
  const [adding, setAdding] = useState(false);
  const [deleting, setDeleting] = useState<OverrideView | null>(null);

  return (
    <div className="panel">
      <div className="panel__head">
        <h2 className="panel__title">{t("overrides.title")}</h2>
        <button type="button" className="button button--primary" onClick={() => setAdding(true)} disabled={view.staff.length === 0}>
          {t("overrides.add")}
        </button>
      </div>
      <p className="panel__intro">{view.staff.length === 0 ? t("overrides.noStaff") : t("overrides.intro")}</p>
      {view.overrides.length === 0 ? (
        <p className="panel__empty">{t("overrides.empty")}</p>
      ) : (
        <ul className="list">
          {view.overrides.map((o) => (
            <li key={o.id} className="list__item">
              <div className="list__main">
                <p className="list__title">
                  {formatShortDate(o.businessDate)} · {o.firstName} {o.lastName}
                </p>
                <p className="list__detail">
                  {o.kind === "off" ? t("overrides.off") : o.blocks.map((b) => formatHours(t, b)).join(", ")}
                </p>
              </div>
              <div className="list__actions">
                <button type="button" className="button button--quiet button--danger-text" onClick={() => setDeleting(o)}>
                  {t("common.delete")}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {adding && <OverrideEditor onClose={() => setAdding(false)} />}
      {deleting && (
        <Confirm
          title={t("overrides.deleteTitle")}
          confirmLabel={t("overrides.deleteYes")}
          danger
          onConfirm={() => run(() => api.overrideDelete(deleting.id))}
          onClose={() => setDeleting(null)}
        >
          <p>
            {t("overrides.deleteBody", {
              name: `${deleting.firstName} ${deleting.lastName}`,
              date: formatShortDate(deleting.businessDate),
            })}
          </p>
        </Confirm>
      )}
    </div>
  );
}

function OverrideEditor({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const { view, run } = useAdmin();
  const [staffId, setStaffId] = useState(view.staff[0]?.id ?? "");
  const [date, setDate] = useState(view.today);
  const [kind, setKind] = useState<OverrideKind>("off");
  const [blocks, setBlocks] = useState<EditBlock[]>(() => [{ uid: newUid(), id: null, start: "", end: "" }]);
  const [report, setReport] = useState<OverrideReport | null>(null);
  const [touched, setTouched] = useState<Set<string>>(() => new Set());
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const drafts: RangeDraft[] = useMemo(() => blocks.map((b) => ({ id: b.id, start: b.start, end: b.end })), [blocks]);

  useEffect(() => {
    let current = true;
    api
      .validateOverride(kind, date, drafts)
      .then((r) => current && setReport(r))
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [kind, date, drafts]);

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    setSubmitted(true);
    setError(null);
    if (!report || !report.ok) {
      if (report?.noBlocks) setError(t("staff.noBlocks"));
      else if (report && !report.dateProblem) setError(t("errors.blocks"));
      return;
    }
    setBusy(true);
    try {
      await run(() => api.overrideSave(staffId, date, kind, kind === "off" ? [] : drafts));
      onClose();
    } catch (e) {
      setError(errorMessage(t, e));
      setBusy(false);
    }
  };

  const dateError = report?.dateProblem ? t(`overrides.${report.dateProblem}`) : null;

  return (
    <Dialog title={t("overrides.newTitle")} onClose={busy ? undefined : onClose} wide>
      <form className="form" onSubmit={(e) => void save(e)}>
        <div className="form__row">
          <Field label={t("overrides.person")}>
            <select className="input" value={staffId} onChange={(e) => setStaffId(e.target.value)} disabled={busy} data-autofocus>
              {view.staff.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.firstName} {s.lastName}
                </option>
              ))}
            </select>
          </Field>
          <Field label={t("overrides.date")} error={dateError}>
            <input className="input" type="date" min={view.today} value={date} onChange={(e) => setDate(e.target.value)} disabled={busy} />
          </Field>
        </div>
        <fieldset className="radio-group">
          <legend className="field__label">{t("overrides.kind")}</legend>
          {(["off", "replace"] as const).map((k) => (
            <label key={k} className="radio">
              <input type="radio" name="override-kind" value={k} checked={kind === k} onChange={() => setKind(k)} disabled={busy} />
              {t(`overrides.${k}`)}
            </label>
          ))}
        </fieldset>
        {kind === "replace" && (
          <fieldset className="week">
            <legend className="week__legend">{t("overrides.hours")}</legend>
            <BlockList
              blocks={blocks}
              reports={blocks.map((_, i) => report?.blocks[i])}
              showProblems={(uid) => submitted || touched.has(uid)}
              describe={(index) => (blocks[index] ? `${blocks[index].start}–${blocks[index].end}` : "")}
              indexOf={(uid) => blocks.findIndex((b) => b.uid === uid)}
              onChange={(uid, field, value) => setBlocks((all) => all.map((b) => (b.uid === uid ? { ...b, [field]: value } : b)))}
              onBlur={(uid) => setTouched((s) => (s.has(uid) ? s : new Set(s).add(uid)))}
              onRemove={(uid) => setBlocks((all) => all.filter((b) => b.uid !== uid))}
              groupLabel={t("overrides.hours")}
              disabled={busy}
            />
            <button
              type="button"
              className="button button--quiet button--small"
              onClick={() => setBlocks((all) => [...all, { uid: newUid(), id: null, start: "", end: "" }])}
              disabled={busy}
            >
              {t("staff.addHours")}
            </button>
          </fieldset>
        )}
        <p className="field__hint">{t("overrides.replaceNote")}</p>
        {error && <FormMessage tone="error">{error}</FormMessage>}
        <div className="dialog__actions">
          <button type="button" className="button button--quiet" onClick={onClose} disabled={busy}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="button button--primary" disabled={busy}>
            {busy ? t("common.saving") : t("common.save")}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

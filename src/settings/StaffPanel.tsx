import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, toCmdError, type StaffView, type WeekBlockDraft, type WeekReport } from "../api";
import { Confirm } from "../components/Confirm";
import { Dialog } from "../components/Dialog";
import { Field, FormMessage } from "../components/Field";
import { errorMessage, nameProblem } from "../errors";
import { formatHours } from "../format";
import { useAdmin } from "./admin";
import { BlockList } from "./BlockList";
import { newUid, type EditBlock } from "./blocks";

const WEEKDAYS = [1, 2, 3, 4, 5, 6, 7] as const;
type WeekdayKey = `${(typeof WEEKDAYS)[number]}`;

export function StaffPanel() {
  const { t } = useTranslation();
  const { view, run } = useAdmin();
  const [editing, setEditing] = useState<StaffView | "new" | null>(null);
  const [removing, setRemoving] = useState<StaffView | null>(null);

  return (
    <div className="panel">
      <div className="panel__head">
        <h2 className="panel__title">{t("staff.title")}</h2>
        <button type="button" className="button button--primary" onClick={() => setEditing("new")}>
          {t("staff.add")}
        </button>
      </div>
      {view.staff.length === 0 ? (
        <p className="panel__empty">{t("staff.empty")}</p>
      ) : (
        <ul className="list">
          {view.staff.map((person) => (
            <li key={person.id} className="list__item">
              <div className="list__main">
                <p className="list__title">
                  {person.firstName} {person.lastName}
                </p>
                <ul className="week-summary">
                  {WEEKDAYS.map((day) => {
                    const blocks = person.blocks.filter((b) => b.weekday === day);
                    return (
                      <li key={day} className={blocks.length === 0 ? "week-summary__off" : undefined}>
                        <span className="week-summary__day">{t(`weekdayShort.${String(day) as WeekdayKey}`)}</span>{" "}
                        {blocks.length === 0 ? t("staff.noHours") : blocks.map((b) => formatHours(t, b)).join(", ")}
                      </li>
                    );
                  })}
                </ul>
              </div>
              <div className="list__actions">
                <button type="button" className="button" onClick={() => setEditing(person)}>
                  {t("common.edit")}
                </button>
                <button type="button" className="button button--quiet button--danger-text" onClick={() => setRemoving(person)}>
                  {t("common.remove")}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}

      {editing && <StaffEditor person={editing === "new" ? null : editing} onClose={() => setEditing(null)} />}
      {removing && (
        <Confirm
          title={t("staff.removeTitle")}
          confirmLabel={t("staff.removeYes")}
          danger
          onConfirm={() => run(() => api.staffRemove(removing.id))}
          onClose={() => setRemoving(null)}
        >
          <p>{t("staff.removeBody", { name: `${removing.firstName} ${removing.lastName}` })}</p>
        </Confirm>
      )}
    </div>
  );
}

interface WeekEditBlock extends EditBlock {
  weekday: number;
}

function StaffEditor({ person, onClose }: { person: StaffView | null; onClose: () => void }) {
  const { t } = useTranslation();
  const { run } = useAdmin();
  const [firstName, setFirstName] = useState(person?.firstName ?? "");
  const [lastName, setLastName] = useState(person?.lastName ?? "");
  const [blocks, setBlocks] = useState<WeekEditBlock[]>(() =>
    (person?.blocks ?? []).map((b) => ({ uid: newUid(), id: b.id, weekday: b.weekday, start: b.start, end: b.end })),
  );
  const [report, setReport] = useState<WeekReport | null>(null);
  const [touched, setTouched] = useState<Set<string>>(() => new Set());
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<{ first?: string; last?: string }>({});

  const drafts: WeekBlockDraft[] = useMemo(
    () => blocks.map((b) => ({ id: b.id, weekday: b.weekday, start: b.start, end: b.end })),
    [blocks],
  );

  useEffect(() => {
    let current = true;
    api
      .validateWeek(drafts)
      .then((r) => current && setReport(r))
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [drafts]);

  const update = (uid: string, field: "start" | "end", value: string) =>
    setBlocks((all) => all.map((b) => (b.uid === uid ? { ...b, [field]: value } : b)));
  const add = (weekday: number) => setBlocks((all) => [...all, { uid: newUid(), id: null, weekday, start: "", end: "" }]);
  const remove = (uid: string) => setBlocks((all) => all.filter((b) => b.uid !== uid));
  const touch = (uid: string) => setTouched((s) => (s.has(uid) ? s : new Set(s).add(uid)));

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    setSubmitted(true);
    setError(null);
    setFieldErrors({});
    if (report && !report.ok) {
      setError(report.noBlocks ? t("staff.noBlocks") : t("errors.blocks"));
      return;
    }
    setBusy(true);
    try {
      await run(() => api.staffSave(person?.id ?? null, firstName, lastName, drafts));
      onClose();
    } catch (e) {
      const err = toCmdError(e);
      if (err.kind === "invalid" && err.field === "first_name") {
        setFieldErrors({ first: nameProblem(t, err.problem, err.character) });
      } else if (err.kind === "invalid" && err.field === "last_name") {
        setFieldErrors({ last: nameProblem(t, err.problem, err.character) });
      } else {
        setError(errorMessage(t, err));
      }
      setBusy(false);
    }
  };

  const title = person ? t("staff.editTitle", { name: `${person.firstName} ${person.lastName}` }) : t("staff.newTitle");

  return (
    <Dialog title={title} onClose={busy ? undefined : onClose} wide>
      <form className="form" onSubmit={(e) => void save(e)}>
        <div className="form__row">
          <Field label={t("staff.firstName")} error={fieldErrors.first}>
            <input className="input" value={firstName} maxLength={60} onChange={(e) => setFirstName(e.target.value)} disabled={busy} data-autofocus />
          </Field>
          <Field label={t("staff.lastName")} error={fieldErrors.last}>
            <input className="input" value={lastName} maxLength={60} onChange={(e) => setLastName(e.target.value)} disabled={busy} />
          </Field>
        </div>
        <fieldset className="week">
          <legend className="week__legend">{t("staff.schedule")}</legend>
          <p className="field__hint">{t("staff.scheduleHint")}</p>
          {WEEKDAYS.map((day) => {
            const dayLabel = t(`weekday.${String(day) as WeekdayKey}`);
            const dayBlocks = blocks.filter((b) => b.weekday === day);
            return (
              <div key={day} className="week__day">
                <div className="week__label">{dayLabel}</div>
                <div className="week__blocks">
                  {dayBlocks.length === 0 && <span className="week__off">{t("staff.noHours")}</span>}
                  <BlockList
                    blocks={dayBlocks}
                    reports={dayBlocks.map((b) => report?.blocks[blocks.indexOf(b)])}
                    showProblems={(uid) => submitted || touched.has(uid)}
                    describe={(index) => (blocks[index] ? `${blocks[index].start}–${blocks[index].end}` : "")}
                    indexOf={(uid) => dayBlocks.findIndex((b) => b.uid === uid)}
                    onChange={update}
                    onBlur={touch}
                    onRemove={remove}
                    groupLabel={dayLabel}
                    disabled={busy}
                  />
                  <button type="button" className="button button--quiet button--small" onClick={() => add(day)} disabled={busy}>
                    {t("staff.addHours")}
                  </button>
                </div>
              </div>
            );
          })}
        </fieldset>
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

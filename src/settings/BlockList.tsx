import { useTranslation } from "react-i18next";
import type { BlockReport } from "../api";
import { TimeInput } from "../components/TimeInput";
import type { EditBlock } from "./blocks";

interface BlockListProps {
  blocks: EditBlock[];
  /** Reports in the same order as `blocks`. */
  reports: (BlockReport | undefined)[];
  /** Blocks whose problems may be shown (touched, or after a save attempt). */
  showProblems: (uid: string) => boolean;
  /** Describes block `index` of the whole list, for overlap messages. */
  describe: (index: number) => string;
  /** Index of each block in the whole list sent for checking. */
  indexOf: (uid: string) => number;
  onChange: (uid: string, field: "start" | "end", value: string) => void;
  onBlur: (uid: string) => void;
  onRemove: (uid: string) => void;
  /** Label for the group, e.g. the weekday (for screen readers). */
  groupLabel: string;
  disabled?: boolean;
}

/** Start–end rows with "(+1)" and a plain-language problem under each. */
export function BlockList(props: BlockListProps) {
  const { t } = useTranslation();
  const { blocks, reports, showProblems, describe, indexOf, onChange, onBlur, onRemove, groupLabel, disabled } = props;
  return (
    <ul className="block-list">
      {blocks.map((block, i) => {
        const report = reports[i];
        const problem = report?.problem && showProblems(block.uid) ? report.problem : null;
        const message =
          problem === "overlaps" && report?.other != null
            ? t("block.overlaps", { other: describe(report.other) })
            : problem
              ? t(`block.${problem}`)
              : null;
        return (
          <li key={block.uid} className={`block-row${message ? " block-row--error" : ""}`}>
            <div className="block-row__times" role="group" aria-label={`${groupLabel} ${indexOf(block.uid) + 1}`}>
              <label className="block-row__field">
                <span className="visually-hidden">{t("staff.start")}</span>
                <TimeInput
                  value={block.start}
                  onChange={(v) => onChange(block.uid, "start", v)}
                  onBlur={() => onBlur(block.uid)}
                  disabled={disabled}
                  aria-invalid={message ? true : undefined}
                />
                {report?.startNextDay && <span className="plus-one" title={t("common.plusOneHint")}>{t("common.plusOne")}</span>}
              </label>
              <span className="block-row__dash" aria-hidden="true">
                {t("common.dash")}
              </span>
              <label className="block-row__field">
                <span className="visually-hidden">{t("staff.end")}</span>
                <TimeInput
                  value={block.end}
                  onChange={(v) => onChange(block.uid, "end", v)}
                  onBlur={() => onBlur(block.uid)}
                  disabled={disabled}
                  aria-invalid={message ? true : undefined}
                />
                {report?.endNextDay && <span className="plus-one" title={t("common.plusOneHint")}>{t("common.plusOne")}</span>}
              </label>
              <button
                type="button"
                className="icon-button"
                onClick={() => onRemove(block.uid)}
                disabled={disabled}
                aria-label={t("staff.removeHours")}
                title={t("staff.removeHours")}
              >
                ×
              </button>
            </div>
            {message && (
              <p className="field__error" role="alert">
                {message}
              </p>
            )}
          </li>
        );
      })}
    </ul>
  );
}

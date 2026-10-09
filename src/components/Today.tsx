import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  api,
  type AlarmBanner,
  type Banners as BannerState,
  type MarkKind,
  type Platform,
  type RefusedMark,
  type Row,
  type RowStatus,
  type TodayView,
} from "../api";
import { errorMessage } from "../errors";
import { formatDate, formatLongDate, formatStamp } from "../dates";
import { formatHours } from "../format";
import { Dialog } from "./Dialog";
import { FormMessage } from "./Field";
import "./Today.css";

const STATUS_CLASS: Record<RowStatus, string> = {
  pending: "row--pending",
  late: "row--due",
  checked_in: "row--in",
  should_have_left: "row--late-out",
  left: "row--left",
};

const STATUS_LABEL = {
  pending: "status.pending",
  late: "status.due",
  checked_in: "status.checkedIn",
  should_have_left: "status.shouldHaveLeft",
  left: "status.left",
} as const;

interface TodayProps {
  today: TodayView | null;
  banners: BannerState;
  platform: Platform;
  /** Android: a permission the alarms need is missing (SPEC §6 banner). */
  permissionsMissing: boolean;
  onFixPermissions: () => void;
  /** Android: a ring-mode alarm in progress on this phone. */
  alarm?: AlarmBanner | null;
  onStopAlarm?: () => void;
  /** Called after a mark so the screen updates at once. */
  onChanged: () => void;
}

/** The main screen (SPEC §6). */
export function Today({
  today,
  banners,
  platform,
  permissionsMissing,
  onFixPermissions,
  alarm = null,
  onStopAlarm,
  onChanged,
}: TodayProps) {
  const { t } = useTranslation();
  const [confirm, setConfirm] = useState<{ row: Row; kind: MarkKind } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!confirm) return;
    setBusy(true);
    try {
      await api.mark(confirm.row, confirm.kind);
      setConfirm(null);
      onChanged();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  };

  const heading = today ? formatLongDate(today.businessDate) : null;
  const name = confirm ? `${confirm.row.firstName} ${confirm.row.lastName}` : "";

  return (
    <section className="today" aria-labelledby="today-heading">
      {alarm && <AlarmBar alarm={alarm} onStop={() => onStopAlarm?.()} />}
      <div className="today__head">
        <h1 id="today-heading" className="today__date">
          {heading && (
            <>
              <span className="today__weekday">{heading.weekday}</span>
              <span className="today__day">{heading.date}</span>
            </>
          )}
        </h1>
        {today && (
          <time className="today__clock" aria-live="off">
            {today.clock}
          </time>
        )}
      </div>

      <Banners
        banners={banners}
        today={today?.businessDate ?? null}
        phone={platform === "android"}
        permissionsMissing={permissionsMissing}
        onFixPermissions={onFixPermissions}
        onChanged={onChanged}
      />

      {today === null ? (
        <p className="today__empty">{banners.offline ? t("today.noData") : t("today.syncing")}</p>
      ) : today.rows.length === 0 ? (
        <p className="today__empty">{t("today.empty")}</p>
      ) : (
        <ul className="rows">
          {today.rows.map((row) => (
            <li key={row.key}>
              <RowItem
                row={row}
                onTap={(kind) => {
                  setError(null);
                  setConfirm({ row, kind });
                }}
              />
            </li>
          ))}
        </ul>
      )}

      {confirm && (
        <Dialog
          title={confirm.kind === "in" ? t("confirm.inTitle") : t("confirm.outTitle")}
          onClose={() => !busy && setConfirm(null)}
          actions={
            <>
              <button type="button" className="button button--quiet button--large" onClick={() => setConfirm(null)} disabled={busy}>
                {t("common.cancel")}
              </button>
              <button
                type="button"
                className={`button button--large ${confirm.kind === "in" ? "button--go" : "button--primary"}`}
                onClick={() => void submit()}
                disabled={busy}
                data-autofocus
              >
                {confirm.kind === "in" ? t("confirm.inYes") : t("confirm.outYes")}
              </button>
            </>
          }
        >
          <p className="confirm__question">
            {confirm.kind === "in" ? t("confirm.inBody", { name }) : t("confirm.outBody", { name })}
          </p>
          <p className="confirm__hours">{formatHours(t, confirm.row)}</p>
          <p className="confirm__note">{t("confirm.noUndo")}</p>
          {error && <FormMessage tone="error">{error}</FormMessage>}
        </Dialog>
      )}
    </section>
  );
}

function RowItem({ row, onTap }: { row: Row; onTap: (kind: MarkKind) => void }) {
  const { t } = useTranslation();
  const content = (
    <>
      <span className="row__name">
        <span className="row__first">{row.firstName}</span> <span className="row__last">{row.lastName}</span>
      </span>
      <span className="row__hours">{formatHours(t, row)}</span>
      <span className="row__status">{t(STATUS_LABEL[row.status])}</span>
    </>
  );
  const className = `row ${STATUS_CLASS[row.status]}`;
  // A row whose person already left does nothing when tapped (SPEC §6).
  if (row.nextMark === null) {
    return <div className={className}>{content}</div>;
  }
  const kind = row.nextMark;
  return (
    <button type="button" className={`${className} row--tappable`} onClick={() => onTap(kind)}>
      {content}
    </button>
  );
}

/**
 * Android: the alarm ringing on this phone (or silent between rings), with
 * a large «Σταμάτημα», so it can be stopped even if its notification was
 * swiped away. Like the alarm screen's button, it marks nobody.
 */
function AlarmBar({ alarm, onStop }: { alarm: AlarmBanner; onStop: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="alarm-bar" role="alert">
      <p className="alarm-bar__title">{alarm.ringing ? t("today.alarmRinging") : t("today.alarmSilent")}</p>
      {alarm.checkIn.length > 0 && (
        <p className="alarm-bar__names">
          {t("alarm.checkIn")}: {alarm.checkIn.join(", ")}
        </p>
      )}
      {alarm.checkOut.length > 0 && (
        <p className="alarm-bar__names">
          {t("alarm.checkOut")}: {alarm.checkOut.join(", ")}
        </p>
      )}
      <button type="button" className="button button--primary button--large alarm-bar__stop" onClick={onStop}>
        {t("alarm.stop")}
      </button>
      <p className="alarm-bar__note">{t("alarm.note")}</p>
    </div>
  );
}

/** A mark the server refused: says so, by name, until «Εντάξει». */
function RefusedNotice({ mark, onChanged }: { mark: RefusedMark; onChanged: () => void }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const name = `${mark.firstName} ${mark.lastName}`.trim() || t("banner.refusedUnknownName");
  const kind = mark.kind === "in" ? t("marks.in") : t("marks.out");
  const text = mark.otherDate
    ? t("banner.refusedOnDate", { kind, name, date: formatDate(mark.otherDate) })
    : t("banner.refused", { kind, name });
  const dismiss = async () => {
    setBusy(true);
    try {
      await api.dismissRefusedMark(mark.id);
      onChanged();
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="banner banner--refused" role="alert">
      <p className="banner__text">{text}</p>
      <button type="button" className="button" onClick={() => void dismiss()} disabled={busy}>
        {t("common.ok")}
      </button>
    </div>
  );
}

function Banners({
  banners,
  today,
  phone,
  permissionsMissing,
  onFixPermissions,
  onChanged,
}: {
  banners: BannerState;
  today: string | null;
  /** Android wording ("this phone") instead of Windows. */
  phone: boolean;
  permissionsMissing: boolean;
  onFixPermissions: () => void;
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const items: { key: string; text: string }[] = [];
  if (banners.offline) {
    items.push({
      key: "offline",
      text: banners.lastSync
        ? t("banner.offline", { when: formatStamp(banners.lastSync, today) })
        : t("banner.offlineNever"),
    });
  }
  if (banners.soundOff) items.push({ key: "sound", text: t("banner.soundOff") });
  if (banners.clockSkew) items.push({ key: "skew", text: t(phone ? "banner.clockSkewPhone" : "banner.clockSkew") });
  if (banners.horizonShort) items.push({ key: "horizon", text: t(phone ? "banner.horizonShortPhone" : "banner.horizonShort") });
  if (items.length === 0 && banners.refusedMarks.length === 0 && !permissionsMissing) return null;
  return (
    <div className="banners">
      {permissionsMissing && (
        <div className="banner banner--permissions" role="alert">
          <p className="banner__text">{t("banner.permissions")}</p>
          <button type="button" className="button button--primary" onClick={onFixPermissions}>
            {t("banner.fix")}
          </button>
        </div>
      )}
      {banners.refusedMarks.map((mark) => (
        <RefusedNotice key={mark.id} mark={mark} onChanged={onChanged} />
      ))}
      {items.map((item) => (
        <p key={item.key} className={`banner banner--${item.key}`} role="status">
          {item.text}
        </p>
      ))}
    </div>
  );
}

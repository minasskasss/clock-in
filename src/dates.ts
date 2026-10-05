/**
 * Every date the app shows or reads, in one place (SPEC §4.2).
 *
 * - On screen, numeric dates are always dd/mm/yyyy and times 24-hour HH:MM,
 *   whatever the Windows locale says. No native date pickers: they follow
 *   the system locale (mm/dd/yyyy, AM/PM on a US setup).
 * - Inside the app and on the server, dates stay ISO ("2026-10-05").
 *
 * This only reformats text. Whether a date exists (31/02) and every time
 * calculation is checked in Rust.
 */
import type { LocalStamp } from "./api";

/** Weekday and month names are always Greek (SPEC §3). */
const LOCALE = "el-GR";

const ISO_DATE = /^(\d{4})-(\d{2})-(\d{2})$/;
const ISO_DATE_TIME = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})$/;
/** d/m/yyyy, with "/", "." or "-" between the parts. */
const TYPED_DATE = /^(\d{1,2})[/.-](\d{1,2})[/.-](\d{4})$/;
const TYPED_TIME = /^(\d{1,2}):(\d{2})$/;

const pad = (n: number) => String(n).padStart(2, "0");

/** "2026-10-05" → "05/10/2026". Anything else is returned unchanged. */
export function formatDate(iso: string): string {
  const m = ISO_DATE.exec(iso);
  return m ? `${m[3]}/${m[2]}/${m[1]}` : iso;
}

/**
 * A typed day-first date → ISO: "05/10/2026" and "5/10/2026" both mean
 * 5 October 2026. Null if it isn't day/month/year in range.
 */
export function parseDate(text: string): string | null {
  const m = TYPED_DATE.exec(text.trim());
  if (!m) return null;
  const day = Number(m[1]);
  const month = Number(m[2]);
  if (day < 1 || day > 31 || month < 1 || month > 12) return null;
  return `${m[3]}-${pad(month)}-${pad(day)}`;
}

/** "2026-10-05T21:00" → "05/10/2026 21:00". */
export function formatDateTime(iso: string): string {
  const m = ISO_DATE_TIME.exec(iso);
  return m?.[1] && m[2] ? `${formatDate(m[1])} ${m[2]}` : iso;
}

/** "05/10/2026 21:00" (24-hour) → "2026-10-05T21:00". Null if not that shape. */
export function parseDateTime(text: string): string | null {
  const [datePart = "", timePart = "", ...rest] = text.trim().split(/\s+/);
  if (rest.length > 0) return null;
  const date = parseDate(datePart);
  const time = TYPED_TIME.exec(timePart);
  if (!date || !time) return null;
  const hour = Number(time[1]);
  const minute = Number(time[2]);
  if (hour > 23 || minute > 59) return null;
  return `${date}T${pad(hour)}:${pad(minute)}`;
}

/** A civil ISO date as a calendar day, independent of the device timezone. */
function civil(iso: string): Date {
  return new Date(`${iso}T00:00:00Z`);
}

/** The Today header keeps the written form: "Δευτέρα" / "5 Οκτωβρίου". */
export function formatLongDate(iso: string): { weekday: string; date: string } {
  const day = civil(iso);
  return {
    weekday: new Intl.DateTimeFormat(LOCALE, { weekday: "long", timeZone: "UTC" }).format(day),
    date: new Intl.DateTimeFormat(LOCALE, { day: "numeric", month: "long", timeZone: "UTC" }).format(day),
  };
}

/** "Τρι 06/10/2026": short weekday plus the numeric date (lists of one-off changes). */
export function formatShortDate(iso: string): string {
  const weekday = new Intl.DateTimeFormat(LOCALE, { weekday: "short", timeZone: "UTC" }).format(civil(iso));
  return `${weekday} ${formatDate(iso)}`;
}

/** A stamp as "14:03" when it is on `today`, else "04/10/2026 14:03". */
export function formatStamp(stamp: LocalStamp, today: string | null): string {
  return stamp.date === today ? stamp.time : `${formatDate(stamp.date)} ${stamp.time}`;
}

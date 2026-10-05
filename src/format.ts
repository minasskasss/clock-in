/**
 * Display formatting only. Every date and time arrives from Rust already in
 * Greek wall-clock terms; nothing here computes times.
 */
import type { TFunction } from "i18next";
import type { LocalStamp } from "./api";

/** Dates are always shown in Greek (SPEC §3). */
const LOCALE = "el-GR";

/** A civil ISO date ("2026-10-05") as a calendar day, independent of the device timezone. */
function civil(iso: string): Date {
  return new Date(`${iso}T00:00:00Z`);
}

/** "Δευτέρα 5 Οκτωβρίου". */
export function formatLongDate(iso: string): { weekday: string; date: string } {
  const day = civil(iso);
  return {
    weekday: new Intl.DateTimeFormat(LOCALE, { weekday: "long", timeZone: "UTC" }).format(day),
    date: new Intl.DateTimeFormat(LOCALE, { day: "numeric", month: "long", timeZone: "UTC" }).format(day),
  };
}

/** "Τρι 6 Οκτ". */
export function formatShortDate(iso: string): string {
  return new Intl.DateTimeFormat(LOCALE, {
    weekday: "short",
    day: "numeric",
    month: "short",
    timeZone: "UTC",
  }).format(civil(iso));
}

/** A stamp as "14:03" when it is on `today`, else "5 Οκτ 14:03". */
export function formatStamp(stamp: LocalStamp, today: string | null): string {
  if (stamp.date === today) return stamp.time;
  const date = new Intl.DateTimeFormat(LOCALE, {
    day: "numeric",
    month: "short",
    timeZone: "UTC",
  }).format(civil(stamp.date));
  return `${date} ${stamp.time}`;
}

/** Seconds as "m:ss". */
export function formatCountdown(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** "12:00 – 02:00 (+1)" pieces, with "(+1)" where a time falls after midnight. */
export function formatHours(
  t: TFunction,
  block: { start: string; end: string; startNextDay: boolean; endNextDay: boolean },
): string {
  const plus = ` ${t("common.plusOne")}`;
  return `${block.start}${block.startNextDay ? plus : ""} ${t("common.dash")} ${block.end}${block.endNextDay ? plus : ""}`;
}

/**
 * Tidies a typed time into "HH:MM" where the intent is clear ("9" → "09:00",
 * "930" → "09:30", "1730" → "17:30", "9:5" stays as typed). Text that isn't a
 * time is left alone; Rust decides whether it is valid.
 */
export function tidyTime(text: string): string {
  const raw = text.trim().replace(/[.,]/g, ":");
  const match = /^(\d{1,2})$/.exec(raw) ?? /^(\d{1,2}):?(\d{2})$/.exec(raw);
  if (!match?.[1]) return raw;
  return `${match[1].padStart(2, "0")}:${match[2] ?? "00"}`;
}

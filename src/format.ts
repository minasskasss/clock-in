/**
 * Display formatting only (dates are in `dates.ts`). Every time arrives from
 * Rust already in Greek wall-clock terms; nothing here computes times.
 */
import type { TFunction } from "i18next";
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

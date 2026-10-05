import type { TFunction } from "i18next";
import { toCmdError, type CmdError } from "./api";
import { formatCountdown } from "./format";

const SERVER_CODES = [
  "already_initialized",
  "not_initialized",
  "bad_passphrase",
  "no_session",
  "revoked",
  "bad_secret",
  "invalid_input",
] as const;

/** A plain-language message for any command error. */
export function errorMessage(t: TFunction, error: unknown): string {
  const e: CmdError = toCmdError(error);
  switch (e.kind) {
    case "offline":
      return t("errors.offline");
    case "not_paired":
      return t("errors.notPaired");
    case "not_configured":
      return t("errors.notConfigured");
    case "internal":
      return t("errors.internal");
    case "rejected":
      if (e.code === "locked") return t("lockout.locked", { time: formatCountdown(e.retryAfterS ?? 0) });
      if ((SERVER_CODES as readonly string[]).includes(e.code)) {
        return t(`errors.${e.code as (typeof SERVER_CODES)[number]}`);
      }
      return t("errors.generic");
    case "invalid":
      return invalidMessage(t, e);
  }
}

function invalidMessage(t: TFunction, e: Extract<CmdError, { kind: "invalid" }>): string {
  switch (e.field) {
    case "passphrase":
      return passphraseProblem(t, e.problem, e.positions);
    case "first_name":
    case "last_name":
      return nameProblem(t, e.problem, e.character);
    case "business_date":
      return e.problem === "past_date" ? t("overrides.past_date") : t("overrides.bad_date");
    case "device_name":
      return t("errors.device_name");
    case "quit_code":
      return t("errors.quit_code");
    case "blocks":
      return t("errors.blocks");
    default:
      return t("errors.range");
  }
}

/** The message for a passphrase problem; positions are 0-based. */
export function passphraseProblem(t: TFunction, problem: string, positions?: number[]): string {
  switch (problem) {
    case "too_few_words":
      return t("passphrase.too_few_words");
    case "too_short":
      return t("passphrase.too_short");
    case "unknown_words":
      return t("passphrase.unknown_words", { positions: (positions ?? []).map((p) => p + 1).join(", ") });
    default:
      return t("passphrase.empty");
  }
}

export function nameProblem(t: TFunction, problem: string, character?: string): string {
  switch (problem) {
    case "too_long":
      return t("name.too_long");
    case "no_letters":
      return t("name.no_letters");
    case "invalid_character":
      return t("name.invalid_character", { character: character ?? "" });
    default:
      return t("name.empty");
  }
}

/** The field an `invalid` error is about, if any. */
export function invalidField(error: unknown): string | null {
  const e = toCmdError(error);
  return e.kind === "invalid" ? e.field : null;
}

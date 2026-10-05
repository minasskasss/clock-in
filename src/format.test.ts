import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { errorMessage } from "./errors";
import { formatCountdown, formatHours, tidyTime } from "./format";

describe("format", () => {
  it("tidies typed times into HH:MM", () => {
    expect(tidyTime("9")).toBe("09:00");
    expect(tidyTime("930")).toBe("09:30");
    expect(tidyTime("1730")).toBe("17:30");
    expect(tidyTime("9:05")).toBe("09:05");
    expect(tidyTime(" 19.00 ")).toBe("19:00");
    expect(tidyTime("")).toBe("");
    expect(tidyTime("abc")).toBe("abc");
    expect(tidyTime("9:5")).toBe("9:5");
  });

  it("formats countdowns and hours", () => {
    expect(formatCountdown(0)).toBe("0:00");
    expect(formatCountdown(61)).toBe("1:01");
    expect(formatCountdown(3600)).toBe("60:00");
    const t = i18n.getFixedT("el");
    expect(formatHours(t, { start: "19:00", end: "02:00", startNextDay: false, endNextDay: true })).toBe("19:00 – 02:00 (+1)");
    expect(formatHours(t, { start: "01:00", end: "04:00", startNextDay: true, endNextDay: true })).toBe(
      "01:00 (+1) – 04:00 (+1)",
    );
  });

  it("turns command errors into plain messages", () => {
    const t = i18n.getFixedT("el");
    expect(errorMessage(t, { kind: "offline" })).toMatch(/Δεν υπάρχει σύνδεση/);
    expect(errorMessage(t, { kind: "rejected", code: "locked", retryAfterS: 120, details: null })).toBe(
      "Πολλές λάθος προσπάθειες. Δοκιμάστε ξανά σε 2:00.",
    );
    expect(errorMessage(t, { kind: "rejected", code: "something_new", retryAfterS: null, details: null })).toBe(
      "Η αλλαγή δεν έγινε. Δοκιμάστε ξανά.",
    );
    expect(errorMessage(t, { kind: "invalid", field: "first_name", problem: "invalid_character", character: "7" })).toMatch(/«7»/);
    expect(errorMessage(t, { kind: "invalid", field: "business_date", problem: "past_date" })).toBe(
      "Η ημερομηνία πρέπει να είναι σήμερα ή αργότερα.",
    );
    expect(errorMessage(t, "weird")).toBe("Κάτι πήγε στραβά σε αυτόν τον υπολογιστή. Δοκιμάστε ξανά.");
  });
});

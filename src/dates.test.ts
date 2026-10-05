import { describe, expect, it } from "vitest";
import {
  formatDate,
  formatDateTime,
  formatLongDate,
  formatShortDate,
  formatStamp,
  parseDate,
  parseDateTime,
} from "./dates";

describe("dates", () => {
  it("shows numeric dates as dd/mm/yyyy", () => {
    expect(formatDate("2026-10-05")).toBe("05/10/2026");
    expect(formatDate("2026-12-31")).toBe("31/12/2026");
    expect(formatDateTime("2026-10-05T21:00")).toBe("05/10/2026 21:00");
  });

  it("reads a typed date day first: 05/10/2026 is 5 October, never 10 May", () => {
    expect(parseDate("05/10/2026")).toBe("2026-10-05");
    expect(parseDate("5/10/2026")).toBe("2026-10-05");
    expect(parseDate(" 5.10.2026 ")).toBe("2026-10-05");
    expect(parseDate("05-10-2026")).toBe("2026-10-05");
    expect(parseDate("25/10/2026")).toBe("2026-10-25");
    // Month first would be month 25: refused, not swapped.
    expect(parseDate("10/25/2026")).toBeNull();
  });

  it("refuses anything that isn't day/month/year", () => {
    for (const text of ["", "2026-10-05", "05/10/26", "05/10", "32/01/2026", "00/01/2026", "05/13/2026", "05/00/2026", "abc"]) {
      expect(parseDate(text), text).toBeNull();
    }
  });

  it("round-trips", () => {
    for (const iso of ["2026-01-01", "2026-03-29", "2026-10-25", "2027-02-28"]) {
      expect(parseDate(formatDate(iso))).toBe(iso);
    }
    expect(parseDateTime(formatDateTime("2026-10-25T03:30"))).toBe("2026-10-25T03:30");
  });

  it("reads the fake clock as dd/mm/yyyy HH:MM, 24-hour", () => {
    expect(parseDateTime("05/10/2026 21:00")).toBe("2026-10-05T21:00");
    expect(parseDateTime("6/10/2026 4:59")).toBe("2026-10-06T04:59");
    expect(parseDateTime("05/10/2026  00:00")).toBe("2026-10-05T00:00");
    for (const text of ["05/10/2026", "05/10/2026 24:00", "05/10/2026 21:60", "05/10/2026 9:00 PM", "2026-10-05 21:00", "10/25/2026 21:00"]) {
      expect(parseDateTime(text), text).toBeNull();
    }
  });

  it("keeps the written header and numeric short dates, in any device timezone", () => {
    expect(formatLongDate("2026-10-25")).toEqual({ weekday: "Κυριακή", date: "25 Οκτωβρίου" });
    expect(formatShortDate("2026-10-06")).toBe("Τρί 06/10/2026");
  });

  it("shows a stamp's date only when it isn't today", () => {
    expect(formatStamp({ date: "2026-10-05", time: "14:03" }, "2026-10-05")).toBe("14:03");
    expect(formatStamp({ date: "2026-10-04", time: "14:03" }, "2026-10-05")).toBe("04/10/2026 14:03");
    expect(formatStamp({ date: "2026-10-01", time: "10:00" }, null)).toBe("01/10/2026 10:00");
  });
});

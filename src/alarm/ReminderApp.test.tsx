import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { ReminderLine, ReminderView } from "../api";
import { callsTo, mockCommands } from "../test/tauri";
import { ReminderApp } from "./ReminderApp";

function line(overrides: Partial<ReminderLine> = {}): ReminderLine {
  return {
    name: "Μαρία Παππά",
    at: "18:00",
    staffId: "s1",
    sourceBlockId: "b1",
    businessDate: "2026-10-05",
    kind: "in",
    noArrivalMark: false,
    ...overrides,
  };
}

function reminder(overrides: Partial<ReminderView> = {}): ReminderView {
  return {
    groupIds: [7],
    checkIn: [line(), line({ name: "Νίκος Λάμπρου", staffId: "s2", sourceBlockId: "b2", at: "18:05" })],
    checkOut: [line({ name: "Ελένη Ιωάννου", staffId: "s3", sourceBlockId: "b3", at: "17:00", kind: "out", noArrivalMark: true })],
    nextAt: "18:40",
    ...overrides,
  };
}

describe("ReminderApp", () => {
  it("lists only the unmarked names, each with its own time and «Σήμανση»", async () => {
    mockCommands({ reminder_state: { reminder: reminder(), autoDark: false } });
    render(<ReminderApp />);
    expect(await screen.findByText("Λείπει σήμανση")).toBeInTheDocument();
    expect(screen.getByText("Πατήθηκε «Σταμάτημα», αλλά δεν έχει γίνει ακόμα σήμανση για:")).toBeInTheDocument();
    const checkIn = screen.getByRole("region", { name: "Άφιξη" });
    const items = within(checkIn).getAllByRole("listitem");
    expect(items.map((li) => li.textContent)).toEqual(["18:00Μαρία ΠαππάΣήμανση", "18:05Νίκος ΛάμπρουΣήμανση"]);
    const checkOut = screen.getByRole("region", { name: "Αποχώρηση" });
    expect(checkOut).toHaveTextContent("17:00Ελένη Ιωάννουχωρίς σήμανση άφιξης");
    expect(screen.getByRole("status")).toHaveTextContent("Ξαναθυμίζει στις 18:40 αν δεν πατηθεί «Εντάξει».");
    expect(screen.getByText(/μόνο σε αυτή τη συσκευή και δεν σημειώνει κανέναν/)).toBeInTheDocument();
  });

  it("says when it is the last reminder of the day", async () => {
    mockCommands({ reminder_state: { reminder: reminder({ nextAt: null }), autoDark: false } });
    render(<ReminderApp />);
    expect(await screen.findByRole("status")).toHaveTextContent("Τελευταία υπενθύμιση για σήμερα.");
  });

  it("«Σήμανση» asks the same question as the Today list before marking", async () => {
    const calls = mockCommands({ reminder_state: { reminder: reminder(), autoDark: false }, mark: null });
    const user = userEvent.setup();
    render(<ReminderApp />);
    const checkIn = await screen.findByRole("region", { name: "Άφιξη" });
    const [, second] = within(checkIn).getAllByRole("button", { name: "Σήμανση" }) as [HTMLElement, HTMLElement];
    await user.click(second);
    const dialog = screen.getByRole("dialog", { name: "Άφιξη" });
    expect(within(dialog).getByText("Να σημειωθεί ότι ήρθε: Νίκος Λάμπρου;")).toBeInTheDocument();
    expect(callsTo(calls, "mark")).toHaveLength(0);
    await user.click(within(dialog).getByRole("button", { name: "Ναι, ήρθε" }));
    expect(callsTo(calls, "mark")).toEqual([{ staffId: "s2", sourceBlockId: "b2", businessDate: "2026-10-05", kind: "in" }]);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("marks only the departure for a check-out name, and Cancel sends nothing", async () => {
    const calls = mockCommands({ reminder_state: { reminder: reminder(), autoDark: false }, mark: null });
    const user = userEvent.setup();
    render(<ReminderApp />);
    const checkOut = await screen.findByRole("region", { name: "Αποχώρηση" });
    await user.click(within(checkOut).getByRole("button", { name: "Σήμανση" }));
    let dialog = screen.getByRole("dialog", { name: "Αποχώρηση" });
    await user.click(within(dialog).getByRole("button", { name: "Άκυρο" }));
    expect(callsTo(calls, "mark")).toHaveLength(0);
    await user.click(within(checkOut).getByRole("button", { name: "Σήμανση" }));
    dialog = screen.getByRole("dialog", { name: "Αποχώρηση" });
    expect(within(dialog).getByText("Να σημειωθεί ότι έφυγε: Ελένη Ιωάννου;")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Ναι, έφυγε" }));
    expect(callsTo(calls, "mark")).toEqual([{ staffId: "s3", sourceBlockId: "b3", businessDate: "2026-10-05", kind: "out" }]);
  });

  it("«Εντάξει» ends the reminders shown, once", async () => {
    const calls = mockCommands({ reminder_state: { reminder: reminder({ groupIds: [7, 9] }), autoDark: false }, reminder_ok: null });
    const user = userEvent.setup();
    render(<ReminderApp />);
    const ok = await screen.findByRole("button", { name: "Εντάξει" });
    await user.click(ok);
    expect(callsTo(calls, "reminder_ok")).toEqual([{ groupIds: [7, 9] }]);
    expect(ok).toBeDisabled();
  });

  it("shows nothing once nobody is left", async () => {
    const calls = mockCommands({ reminder_state: { reminder: null, autoDark: false } });
    render(<ReminderApp />);
    await waitFor(() => expect(callsTo(calls, "reminder_state").length).toBeGreaterThan(0));
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});

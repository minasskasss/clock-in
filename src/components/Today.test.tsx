import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLanguage } from "../i18n";
import { callsTo, mockCommands, row, today } from "../test/tauri";
import { Today } from "./Today";

const noBanners = { offline: false, lastSync: null, clockSkew: false, horizonShort: false };

describe("Today view", () => {
  beforeEach(async () => {
    await setLanguage("el");
  });

  it("shows the header, every row with its hours and status, and (+1) after midnight", () => {
    render(
      <Today
        today={today([
          row(),
          row({ key: "b2", sourceBlockId: "b2", start: "19:00", end: "02:00", endNextDay: true, status: "late" }),
          row({ key: "b3", firstName: "Νίκος", lastName: "Αλεξίου", status: "checked_in", nextMark: "out" }),
          row({ key: "b4", firstName: "Άννα", lastName: "Βλάχου", status: "should_have_left", nextMark: "out" }),
          row({ key: "b5", firstName: "Γιώργος", lastName: "Δήμου", status: "left", nextMark: null }),
        ])}
        banners={noBanners}
        onChanged={() => {}}
      />,
    );
    const heading = screen.getByRole("heading", { level: 1 });
    expect(heading).toHaveTextContent("Δευτέρα");
    expect(heading).toHaveTextContent("5 Οκτωβρίου");
    expect(screen.getByText("11:42")).toBeInTheDocument();
    expect(screen.getByText("19:00 – 02:00 (+1)")).toBeInTheDocument();
    for (const status of ["Σε αναμονή", "Καθυστερεί", "Ήρθε", "Έπρεπε να έχει φύγει", "Έφυγε"]) {
      expect(screen.getByText(status)).toBeInTheDocument();
    }
    // Five rows, but a person who left can't be tapped.
    expect(screen.getAllByRole("listitem")).toHaveLength(5);
    expect(screen.getAllByRole("button")).toHaveLength(4);
    expect(screen.queryByRole("button", { name: /Γιώργος/ })).not.toBeInTheDocument();
  });

  it("asks before marking a check-in, then sends it", async () => {
    const calls = mockCommands({ mark: null });
    const onChanged = vi.fn();
    const user = userEvent.setup();
    render(<Today today={today()} banners={noBanners} onChanged={onChanged} />);

    await user.click(screen.getByRole("button", { name: /Μαρία Παππά/ }));
    const dialog = screen.getByRole("dialog", { name: "Άφιξη" });
    expect(within(dialog).getByText("Να σημειωθεί ότι ήρθε: Μαρία Παππά;")).toBeInTheDocument();
    expect(callsTo(calls, "mark")).toHaveLength(0);

    await user.click(within(dialog).getByRole("button", { name: "Ναι, ήρθε" }));
    expect(callsTo(calls, "mark")).toEqual([
      { staffId: "s1", sourceBlockId: "b1", businessDate: "2026-10-05", kind: "in" },
    ]);
    expect(onChanged).toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("asks 'left' for a checked-in row, and Cancel sends nothing", async () => {
    const calls = mockCommands({ mark: null });
    const user = userEvent.setup();
    render(<Today today={today([row({ status: "checked_in", nextMark: "out" })])} banners={noBanners} onChanged={() => {}} />);

    await user.click(screen.getByRole("button", { name: /Μαρία Παππά/ }));
    const dialog = screen.getByRole("dialog", { name: "Αποχώρηση" });
    expect(within(dialog).getByText("Να σημειωθεί ότι έφυγε: Μαρία Παππά;")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Άκυρο" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(callsTo(calls, "mark")).toHaveLength(0);
  });

  it("shows the empty state and the loading state", () => {
    const { rerender } = render(<Today today={today([])} banners={noBanners} onChanged={() => {}} />);
    expect(screen.getByText("Κανείς δεν δουλεύει σήμερα")).toBeInTheDocument();
    rerender(<Today today={null} banners={noBanners} onChanged={() => {}} />);
    expect(screen.getByText("Φόρτωση του σημερινού προγράμματος…")).toBeInTheDocument();
    rerender(<Today today={null} banners={{ ...noBanners, offline: true }} onChanged={() => {}} />);
    expect(screen.getByText(/Δεν υπάρχει σύνδεση και δεν έχει φορτωθεί/)).toBeInTheDocument();
  });

  it("shows banners only when relevant", () => {
    const { rerender } = render(<Today today={today()} banners={noBanners} onChanged={() => {}} />);
    expect(screen.queryAllByRole("status")).toHaveLength(0);
    rerender(
      <Today
        today={today()}
        banners={{ offline: true, lastSync: { date: "2026-10-05", time: "11:20" }, clockSkew: true, horizonShort: true }}
        onChanged={() => {}}
      />,
    );
    expect(screen.getByText(/Τελευταίος συγχρονισμός: 11:20\./)).toBeInTheDocument();
    expect(screen.getByText(/διαφέρει πάνω από 2 λεπτά/)).toBeInTheDocument();
    expect(screen.getByText(/λιγότερες από 3 μέρες/)).toBeInTheDocument();
  });
});

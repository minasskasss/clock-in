import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { AlarmName, AlarmView } from "../api";
import { callsTo, mockCommands } from "../test/tauri";
import { AlarmApp } from "./AlarmApp";

const at = (time: string) => (name: string): AlarmName => ({ name, at: time });

function alarm(overrides: Partial<AlarmView> = {}): AlarmView {
  return {
    id: 3,
    at: "09:00",
    checkIn: ["Μαρία Παππά", "Νίκος Λάμπρου"].map(at("09:00")),
    checkOut: ["Ελένη Ιωάννου"].map(at("09:00")),
    ringing: true,
    reringAt: null,
    ...overrides,
  };
}

describe("AlarmApp", () => {
  it("shows the time and the names under «Άφιξη» and «Αποχώρηση»", async () => {
    mockCommands({ alarm_state: { alarm: alarm(), autoDark: false } });
    render(<AlarmApp />);
    expect(await screen.findByText("09:00")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Άφιξη" })).toHaveTextContent("Μαρία ΠαππάΝίκος Λάμπρου");
    expect(screen.getByRole("region", { name: "Αποχώρηση" })).toHaveTextContent("Ελένη Ιωάννου");
    // Ringing: no "silent" notice; Stop is not focused (a stray key press
    // must not silence it).
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Σταμάτημα" })).not.toHaveFocus();
    expect(screen.getByText(/δεν σημειώνει κανέναν/)).toBeInTheDocument();
  });

  it("shows each name's own time when names joined from different minutes", async () => {
    const view = alarm({
      at: null,
      checkIn: [at("04:59")("Μαρία Παππά"), at("05:12")("Νίκος Λάμπρου")],
      checkOut: [at("05:12")("Ελένη Ιωάννου")],
    });
    mockCommands({ alarm_state: { alarm: view, autoDark: false } });
    render(<AlarmApp />);
    const checkIn = await screen.findByRole("region", { name: "Άφιξη" });
    const items = Array.from(checkIn.querySelectorAll("li")).map((li) => li.textContent);
    expect(items).toEqual(["04:59Μαρία Παππά", "05:12Νίκος Λάμπρου"]);
    expect(screen.getByRole("region", { name: "Αποχώρηση" })).toHaveTextContent("05:12Ελένη Ιωάννου");
    // No single time in the header.
    expect(document.querySelector(".alarm__time")).toBeNull();
  });

  it("shows one time in the header when every name shares it", async () => {
    mockCommands({ alarm_state: { alarm: alarm(), autoDark: false } });
    render(<AlarmApp />);
    expect(await screen.findByText("09:00")).toBeInTheDocument();
    expect(document.querySelectorAll(".alarm__name-time")).toHaveLength(0);
  });

  it("hides an empty section", async () => {
    mockCommands({ alarm_state: { alarm: alarm({ checkIn: [] }), autoDark: false } });
    render(<AlarmApp />);
    expect(await screen.findByRole("region", { name: "Αποχώρηση" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Άφιξη" })).not.toBeInTheDocument();
  });

  it("says when a silent alarm rings again", async () => {
    mockCommands({ alarm_state: { alarm: alarm({ ringing: false, reringAt: "09:10" }), autoDark: false } });
    render(<AlarmApp />);
    expect(await screen.findByRole("status")).toHaveTextContent("Ξαναχτυπά στις 09:10");
  });

  it("Stop sends the alarm's id once", async () => {
    const calls = mockCommands({ alarm_state: { alarm: alarm({ id: 7 }), autoDark: false }, alarm_stop: null });
    const user = userEvent.setup();
    render(<AlarmApp />);
    const stop = await screen.findByRole("button", { name: "Σταμάτημα" });
    await user.click(stop);
    expect(callsTo(calls, "alarm_stop")).toEqual([{ id: 7 }]);
    expect(stop).toBeDisabled();
  });

  it("follows the automatic dark theme", async () => {
    mockCommands({ alarm_state: { alarm: alarm(), autoDark: true } });
    render(<AlarmApp />);
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
  });

  it("shows nothing once the alarm ended", async () => {
    const calls = mockCommands({ alarm_state: { alarm: null, autoDark: false } });
    render(<AlarmApp />);
    await waitFor(() => expect(callsTo(calls, "alarm_state").length).toBeGreaterThan(0));
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});

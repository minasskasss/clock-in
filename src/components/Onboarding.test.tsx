import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { PermissionStatus } from "../api";
import { callsTo, mockCommands, today } from "../test/tauri";
import { Onboarding } from "./Onboarding";
import { Today } from "./Today";

function status(overrides: Partial<PermissionStatus> = {}): PermissionStatus {
  return {
    notifications: true,
    exactAlarms: true,
    fullScreen: true,
    battery: true,
    unusedApps: true,
    oem: "",
    oemDone: false,
    ...overrides,
  };
}

describe("Android onboarding checklist", () => {
  it("shows ✓ / ✗ per item and opens the phone's screen for a missing one", async () => {
    const calls = mockCommands({ android_open_settings: null });
    const user = userEvent.setup();
    render(<Onboarding permissions={status({ fullScreen: false })} permissionsOk={false} onDone={() => {}} />);
    const [notifications, , fullScreen, ...rest] = screen.getAllByRole("listitem") as [HTMLElement, HTMLElement, HTMLElement];
    expect(rest).toHaveLength(2);
    expect(within(notifications).getByLabelText("Έγινε")).toHaveTextContent("✓");
    expect(within(fullScreen).getByLabelText("Λείπει")).toHaveTextContent("✗");
    expect(fullScreen).toHaveTextContent("Ειδοποιήσεις σε πλήρη οθόνη");
    await user.click(within(fullScreen).getByRole("button", { name: "Ρύθμιση" }));
    expect(callsTo(calls, "android_open_settings")).toEqual([{ kind: "fullScreen" }]);
    // Not everything granted: it can be put off.
    expect(screen.getByRole("button", { name: "Αργότερα" })).toBeInTheDocument();
  });

  it("adds the Samsung step, which the user confirms by hand", async () => {
    const calls = mockCommands({ android_set_oem_done: null });
    const user = userEvent.setup();
    const onDone = vi.fn();
    render(<Onboarding permissions={status({ oem: "samsung" })} permissionsOk onDone={onDone} />);
    expect(screen.getAllByRole("listitem")).toHaveLength(6);
    expect(screen.getByText("Επιπλέον ρύθμιση για Samsung")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Το έκανα" }));
    expect(callsTo(calls, "android_set_oem_done")).toEqual([{ done: true }]);
    expect(screen.getByText(/Όλα έτοιμα/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Συνέχεια" }));
    expect(onDone).toHaveBeenCalledOnce();
  });

  it("lists the Xiaomi steps, each opening its own screen, confirmed by hand", async () => {
    const calls = mockCommands({ android_open_settings: null, android_set_oem_done: null });
    const user = userEvent.setup();
    render(<Onboarding permissions={status({ oem: "xiaomi" })} permissionsOk onDone={() => {}} />);
    expect(screen.getByText("Επιπλέον ρυθμίσεις για Xiaomi")).toBeInTheDocument();
    expect(screen.getByText(/Αυτόματη εκκίνηση/)).toBeInTheDocument();
    expect(screen.getByText(/Εμφάνιση στην οθόνη κλειδώματος/)).toBeInTheDocument();
    expect(screen.getByText(/Χωρίς περιορισμούς/)).toBeInTheDocument();
    expect(screen.getByText(/λουκέτο/)).toBeInTheDocument();
    for (const button of screen.getAllByRole("button", { name: "Άνοιγμα" })) await user.click(button);
    expect(callsTo(calls, "android_open_settings")).toEqual([
      { kind: "xiaomiAutostart" },
      { kind: "xiaomiPermissions" },
      { kind: "xiaomiBattery" },
    ]);
    expect(screen.queryByRole("button", { name: "Ρύθμιση" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Το έκανα" }));
    expect(callsTo(calls, "android_set_oem_done")).toEqual([{ done: true }]);
  });

  it("says it is checking before Android answered", () => {
    render(<Onboarding permissions={null} permissionsOk onDone={() => {}} />);
    expect(screen.getByText("Έλεγχος…")).toBeInTheDocument();
    expect(screen.queryByRole("listitem")).not.toBeInTheDocument();
  });

  it("puts a fix button on the Today banner when a permission is missing", async () => {
    const user = userEvent.setup();
    const onFix = vi.fn();
    const banners = { offline: false, lastSync: null, clockSkew: true, horizonShort: false, soundOff: false, refusedMarks: [] };
    render(
      <Today today={today()} banners={banners} platform="android" permissionsMissing onFixPermissions={onFix} onChanged={() => {}} />,
    );
    expect(screen.getByText(/Λείπει μια ρύθμιση του τηλεφώνου/)).toBeInTheDocument();
    // Phone wording, not Windows.
    expect(screen.getByText(/ρολόι αυτού του τηλεφώνου/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Διόρθωση" }));
    expect(onFix).toHaveBeenCalledOnce();
  });

  it("shows a large «Σταμάτημα» on Today while an alarm rings on the phone", async () => {
    const user = userEvent.setup();
    const onStop = vi.fn();
    const banners = { offline: false, lastSync: null, clockSkew: false, horizonShort: false, soundOff: false, refusedMarks: [] };
    render(
      <Today
        today={today()}
        banners={banners}
        platform="android"
        permissionsMissing={false}
        onFixPermissions={() => {}}
        alarm={{ ringing: true, checkIn: ["Μαρία Παππά"], checkOut: [] }}
        onStopAlarm={onStop}
        onChanged={() => {}}
      />,
    );
    const bar = screen.getByRole("alert");
    expect(bar).toHaveTextContent("Χτυπά ειδοποίηση για την κάρτα");
    expect(bar).toHaveTextContent("Άφιξη: Μαρία Παππά");
    await user.click(within(bar).getByRole("button", { name: "Σταμάτημα" }));
    expect(onStop).toHaveBeenCalledOnce();
  });
});

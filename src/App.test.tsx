import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import App from "./App";
import { setSystemPrefersDark } from "./test/setup";
import { appState, callsTo, mockCommands } from "./test/tauri";
import { FAILURES_BEFORE_MESSAGE, POLL_MS } from "./useAppState";

describe("App", () => {
  it("shows the Greek first-run choices on an unpaired device, with the background", async () => {
    mockCommands({ app_state: appState({ phase: "unpaired", today: null }) });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Καλώς ήρθατε στο Clock In" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Ρύθμιση ως πρώτη συσκευή/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Σύνδεση αυτής της συσκευής/ })).toBeInTheDocument();
    expect(screen.getByTestId("background")).toHaveAttribute("aria-hidden", "true");
    // No Settings before pairing.
    expect(screen.queryByRole("button", { name: "Ρυθμίσεις" })).not.toBeInTheDocument();
  });

  it("shows the header mark: icon, the drawn «Clock In» and the Greek subtitle", async () => {
    mockCommands({ app_state: appState({ today: { businessDate: "2026-10-05", clock: "11:42", rows: [] } }) });
    const { container } = render(<App />);
    const name = await screen.findByRole("img", { name: "Clock In" });
    expect(name.tagName.toLowerCase()).toBe("svg");
    expect(name.querySelector("path")?.getAttribute("d")).toMatch(/^M/);
    expect(screen.getByText("ΠΡΟΣΕΛΕΥΣΗ · ΑΠΟΧΩΡΗΣΗ")).toBeInTheDocument();
    // The icon is decoration next to the name.
    expect(container.querySelector(".brand__icon")).toHaveAttribute("alt", "");
  });

  it("explains a build without server settings", async () => {
    mockCommands({ app_state: appState({ phase: "not_configured", today: null }) });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Λείπουν οι ρυθμίσεις διακομιστή" })).toBeInTheDocument();
  });

  it("on Android keeps clear of the system bars with Android's own sizes (old WebViews report none)", async () => {
    window.localStorage.setItem("clockin.onboarding", "done");
    const calls = mockCommands({
      app_state: appState({
        platform: "android",
        android: { alertMode: "ring", permissions: null, permissionsOk: true, alarm: null },
      }),
      android_insets: { top: 24, right: 0, bottom: 48, left: 0 },
    });
    render(<App />);
    const root = document.documentElement.style;
    await waitFor(() => expect(root.getPropertyValue("--android-inset-top")).toBe("24px"));
    expect(root.getPropertyValue("--android-inset-bottom")).toBe("48px");
    expect(callsTo(calls, "android_insets")).toHaveLength(1);
    for (const side of ["top", "right", "bottom", "left"]) root.removeProperty(`--android-inset-${side}`);
    window.localStorage.removeItem("clockin.onboarding");
  });

  it("is Greek only: the menu has no language choice", async () => {
    mockCommands({ app_state: appState({ today: { businessDate: "2026-10-05", clock: "11:42", rows: [] } }) });
    const user = userEvent.setup();
    render(<App />);
    expect(await screen.findByText("Κανείς δεν δουλεύει σήμερα")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Δευτέρα");
    await user.click(screen.getByRole("button", { name: "Μενού" }));
    expect(screen.queryByText("Γλώσσα")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /English/ })).not.toBeInTheDocument();
    for (const name of ["Αυτόματο", "Σύστημα", "Φωτεινό", "Σκοτεινό"]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
    expect(screen.getByRole("button", { name: "Αυτόματο" })).toHaveAttribute("aria-pressed", "true");
  });

  it("'Αυτόματο' follows Rust's answer live, without a restart", async () => {
    let autoDark = true;
    mockCommands({ app_state: () => appState({ autoDark }) });
    render(<App />);
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    autoDark = false;
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"), { timeout: 2500 });
    // Remembered, so a restart at night starts dark at once.
    expect(window.localStorage.getItem("clockin.theme.autoDark")).toBe("light");
  });

  it("switches between light and dark from the menu", async () => {
    mockCommands({ app_state: appState() });
    const user = userEvent.setup();
    render(<App />);
    expect(document.documentElement.dataset.theme).toBe("light");

    await user.click(screen.getByRole("button", { name: "Μενού" }));
    await user.click(screen.getByRole("button", { name: "Σκοτεινό" }));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(window.localStorage.getItem("clockin.theme")).toBe("dark");

    await user.click(screen.getByRole("button", { name: "Φωτεινό" }));
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("follows live system theme changes while set to 'System'", () => {
    window.localStorage.setItem("clockin.theme", "system");
    mockCommands({ app_state: appState({ autoDark: true }) });
    render(<App />);
    expect(document.documentElement.dataset.theme).toBe("light");
    act(() => setSystemPrefersDark(true));
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("closes the menu with Escape", async () => {
    mockCommands({ app_state: appState() });
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: "Μενού" }));
    expect(screen.getByRole("button", { name: "Σκοτεινό" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("button", { name: "Σκοτεινό" })).not.toBeInTheDocument();
  });

  it("shows the fake clock only in debug builds", async () => {
    const calls = mockCommands({
      app_state: appState({ debug: { profile: "b", fakeClock: "2026-10-25T03:30", fakeClockSecond: false } }),
      debug_set_clock: null,
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("11:42");
    await user.click(screen.getByRole("button", { name: "Μενού" }));
    expect(screen.getByText("Προφίλ: b")).toBeInTheDocument();
    expect(screen.getByText("Ενεργό: 25/10/2026 03:30")).toBeInTheDocument();
    const box = screen.getByLabelText("Ψεύτικο ρολόι (ώρα Ελλάδας)");
    expect(box).toHaveValue("25/10/2026 03:30");
    await user.clear(box);
    await user.type(box, "05/10/2026 9:00 PM");
    await user.click(screen.getByRole("button", { name: "Ορισμός" }));
    expect(screen.getByText("Γράψτε ημερομηνία και ώρα όπως 05/10/2026 21:00.")).toBeInTheDocument();
    expect(callsTo(calls, "debug_set_clock")).toHaveLength(0);
    await user.clear(box);
    await user.type(box, "05/10/2026 21:00");
    await user.click(screen.getByRole("button", { name: "Ορισμός" }));
    expect(callsTo(calls, "debug_set_clock")).toEqual([{ local: "2026-10-05T21:00", second: false }]);
    await user.click(screen.getByRole("button", { name: "Πραγματική ώρα" }));
    expect(calls.some((c) => c.command === "debug_set_clock" && c.args?.local === null)).toBe(true);
  });

  it("sets the fake clock to the second pass of the repeated hour", async () => {
    const calls = mockCommands({
      app_state: appState({ debug: { profile: null, fakeClock: "2026-10-25T03:20", fakeClockSecond: true } }),
      debug_set_clock: null,
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("11:42");
    await user.click(screen.getByRole("button", { name: "Μενού" }));
    expect(screen.getByText("Ενεργό: 25/10/2026 03:20 (δεύτερη φορά, χειμερινή ώρα)")).toBeInTheDocument();
    const second = screen.getByRole("checkbox", { name: /δεύτερη φορά της ώρας/ });
    expect(second).toBeChecked();
    await user.click(screen.getByRole("button", { name: "Ορισμός" }));
    expect(callsTo(calls, "debug_set_clock")).toEqual([{ local: "2026-10-25T03:20", second: true }]);
    await user.click(second);
    await user.click(screen.getByRole("button", { name: "Ορισμός" }));
    expect(callsTo(calls, "debug_set_clock")[1]).toEqual({ local: "2026-10-25T03:20", second: false });
  });

  it("tells the user when the app core doesn't answer, after a short grace", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      mockCommands({
        app_state: () => {
          throw new Error("no IPC");
        },
      });
      render(<App />);
      // Android starts its core on a thread: the first answers may fail.
      await vi.advanceTimersByTimeAsync(POLL_MS * 2);
      expect(screen.getByText("Φόρτωση…")).toBeInTheDocument();
      await vi.advanceTimersByTimeAsync(POLL_MS * FAILURES_BEFORE_MESSAGE);
      expect(screen.getByText(/Η εφαρμογή δεν αποκρίνεται/)).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});

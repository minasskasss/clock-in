import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import App from "../App";
import { adminView, appState, callsTo, mockCommands } from "../test/tauri";

const noSession = { kind: "rejected", code: "no_session", retryAfterS: null, details: null };

describe("Settings", () => {
  it("asks for the passphrase, says when it is wrong, and opens Settings when right", async () => {
    let unlocked = false;
    const calls = mockCommands({
      app_state: () => appState({ adminUnlocked: unlocked }),
      admin_login: (args: { passphrase: string }) => {
        if (args.passphrase !== "right one") {
          throw { kind: "rejected", code: "bad_passphrase", retryAfterS: null, details: null };
        }
        unlocked = true;
      },
      admin_view: adminView(),
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Ρυθμίσεις" }));
    const dialog = screen.getByRole("dialog", { name: "Ρυθμίσεις" });
    await user.type(within(dialog).getByLabelText("Κωδικός φράση"), "wrong");
    await user.click(within(dialog).getByRole("button", { name: "Άνοιγμα" }));
    expect(await within(dialog).findByText("Λάθος κωδικός φράση.")).toBeInTheDocument();

    await user.type(within(dialog).getByLabelText("Κωδικός φράση"), "right one");
    await user.click(within(dialog).getByRole("button", { name: "Άνοιγμα" }));
    expect(await screen.findByRole("heading", { name: "Ρυθμίσεις", level: 1 })).toBeInTheDocument();
    expect(screen.getByText("Μαρία Παππά")).toBeInTheDocument();
    expect(screen.getByText(/19:00 – 02:00 \(\+1\)/)).toBeInTheDocument();
    expect(callsTo(calls, "admin_login")).toHaveLength(2);
  });

  it("shows the lockout countdown and blocks attempts while it lasts", async () => {
    const calls = mockCommands({ app_state: appState({ lockoutRemainingS: 75 }) });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Ρυθμίσεις" }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText("Πολλές λάθος προσπάθειες. Δοκιμάστε ξανά σε 1:15.")).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Άνοιγμα" })).toBeDisabled();
    expect(callsTo(calls, "admin_login")).toHaveLength(0);
  });

  it("removes a staff member only after confirming", async () => {
    const calls = mockCommands({
      app_state: appState({ adminUnlocked: true }),
      admin_view: adminView(),
      staff_remove: null,
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Ρυθμίσεις" }));
    await user.click(await screen.findByRole("button", { name: "Αφαίρεση" }));
    const dialog = screen.getByRole("dialog", { name: "Αφαίρεση εργαζομένου" });
    expect(within(dialog).getByText(/Να αφαιρεθεί ο\/η Μαρία Παππά;/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Ναι, αφαίρεση" }));
    expect(callsTo(calls, "staff_remove")).toEqual([{ id: "s1" }]);
  });

  it("changes the quit code under Κωδικοί, keeping the other settings, and no longer under Ειδοποιήσεις", async () => {
    const calls = mockCommands({
      app_state: appState({ adminUnlocked: true }),
      admin_view: adminView({ settings: { checkinOffsetMin: -10, checkoutOffsetMin: 5, rollover: "04:00", autostart: false } }),
      settings_save: null,
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Ρυθμίσεις" }));
    await user.click(await screen.findByRole("tab", { name: "Ειδοποιήσεις" }));
    expect(screen.queryByLabelText("Νέος κωδικός εξόδου")).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Κωδικός φράση" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Κωδικοί" }));
    expect(screen.getByRole("heading", { name: "Αλλαγή κωδικού φράσης" })).toBeInTheDocument();
    const submit = screen.getByRole("button", { name: "Αλλαγή κωδικού εξόδου" });
    await user.type(screen.getByLabelText("Νέος κωδικός εξόδου"), "4321");
    await user.type(screen.getByLabelText("Ξανά ο νέος κωδικός εξόδου"), "4320");
    expect(screen.getByText("Οι δύο κωδικοί εξόδου δεν είναι ίδιοι.")).toBeInTheDocument();
    expect(submit).toBeDisabled();
    await user.type(screen.getByLabelText("Ξανά ο νέος κωδικός εξόδου"), "{Backspace}1");
    await user.click(submit);
    expect(await screen.findByText("Ο κωδικός εξόδου άλλαξε.")).toBeInTheDocument();
    expect(callsTo(calls, "settings_save")).toEqual([
      { checkinOffsetMin: -10, checkoutOffsetMin: 5, rollover: "04:00", autostart: false, quitCode: "4321" },
    ]);
  });

  it("hides «Άνοιγμα με την εκκίνηση των Windows» on Android and saves the shop PC's value unchanged", async () => {
    window.localStorage.setItem("clockin.onboarding", "done");
    const calls = mockCommands({
      app_state: appState({
        adminUnlocked: true,
        platform: "android",
        android: { alertMode: "ring", permissions: null, permissionsOk: true, alarm: null },
      }),
      admin_view: adminView({ settings: { checkinOffsetMin: 0, checkoutOffsetMin: 0, rollover: "05:00", autostart: true } }),
      settings_save: null,
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Ρυθμίσεις" }));
    await user.click(await screen.findByRole("tab", { name: "Ειδοποιήσεις" }));
    expect(screen.queryByLabelText("Άνοιγμα με την εκκίνηση των Windows")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Αποθήκευση" }));
    expect(await screen.findByText("Αποθηκεύτηκε.")).toBeInTheDocument();
    expect(callsTo(calls, "settings_save")).toEqual([
      { checkinOffsetMin: 0, checkoutOffsetMin: 0, rollover: "05:00", autostart: true, quitCode: null },
    ]);
    window.localStorage.removeItem("clockin.onboarding");
  });

  it("shows «Άνοιγμα με την εκκίνηση των Windows» on Windows", async () => {
    mockCommands({ app_state: appState({ adminUnlocked: true }), admin_view: adminView() });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Ρυθμίσεις" }));
    await user.click(await screen.findByRole("tab", { name: "Ειδοποιήσεις" }));
    expect(screen.getByLabelText("Άνοιγμα με την εκκίνηση των Windows")).toBeChecked();
  });

  it("shows and reads one-off change dates as dd/mm/yyyy, sending ISO to Rust", async () => {
    const calls = mockCommands({
      app_state: appState({ adminUnlocked: true }),
      admin_view: adminView({
        overrides: [
          { id: "o1", staffId: "s1", firstName: "Μαρία", lastName: "Παππά", businessDate: "2026-10-06", kind: "off", blocks: [] },
        ],
        devices: [
          {
            id: "d1",
            name: "SHOP-PC",
            platform: "windows",
            pairedAt: { date: "2026-10-01", time: "10:00" },
            lastSeen: { date: "2026-10-04", time: "23:41" },
            thisDevice: true,
          },
        ],
      }),
      validate_override: (args: { businessDate: string }) => ({
        dateProblem: args.businessDate === "" ? "bad_date" : null,
        blocks: [],
        noBlocks: false,
        ok: args.businessDate !== "",
      }),
      override_save: null,
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Ρυθμίσεις" }));
    await user.click(await screen.findByRole("tab", { name: "Αλλαγές ημέρας" }));
    expect(screen.getByText(/Τρί 06\/10\/2026 · Μαρία Παππά/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Νέα αλλαγή" }));
    const box = screen.getByLabelText("Ημερομηνία");
    expect(box).toHaveValue("05/10/2026");
    expect(box).not.toHaveAttribute("type", "date");
    await user.clear(box);
    await user.type(box, "10/25/2026");
    await user.tab();
    expect(await screen.findByText("Γράψτε την ημερομηνία ως ηη/μμ/εεεε, π.χ. 05/10/2026.")).toBeInTheDocument();
    await user.clear(box);
    await user.type(box, "07/10/2026");
    await user.click(screen.getByRole("button", { name: "Αποθήκευση" }));
    expect(callsTo(calls, "override_save")).toEqual([{ staffId: "s1", businessDate: "2026-10-07", kind: "off", blocks: [] }]);

    await user.click(screen.getByRole("tab", { name: "Συσκευές" }));
    expect(screen.getByText(/01\/10\/2026 10:00/)).toBeInTheDocument();
    expect(screen.getByText(/04\/10\/2026 23:41/)).toBeInTheDocument();
  });

  it("asks for the passphrase again in place when the server session ran out, then retries", async () => {
    let sessionValid = false;
    const calls = mockCommands({
      app_state: appState({ adminUnlocked: true }),
      admin_view: adminView(),
      admin_login: () => {
        sessionValid = true;
      },
      staff_remove: () => {
        if (!sessionValid) throw noSession;
      },
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Ρυθμίσεις" }));
    await user.click(await screen.findByRole("button", { name: "Αφαίρεση" }));
    await user.click(screen.getByRole("button", { name: "Ναι, αφαίρεση" }));

    expect(await screen.findByText(/Η σύνδεση στις ρυθμίσεις έληξε/)).toBeInTheDocument();
    await user.type(screen.getByLabelText("Κωδικός φράση"), "some words");
    await user.click(screen.getByRole("button", { name: "Άνοιγμα" }));
    await screen.findByRole("heading", { name: "Προσωπικό" });
    expect(callsTo(calls, "staff_remove")).toHaveLength(2);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("checks the weekly schedule while typing and shows (+1)", async () => {
    mockCommands({
      app_state: appState({ adminUnlocked: true }),
      admin_view: adminView({ staff: [] }),
      validate_week: (args: { blocks: { start: string; end: string }[] }) => ({
        blocks: args.blocks.map((b) => ({
          startNextDay: false,
          endNextDay: b.end === "02:00",
          problem: b.start === "" ? "bad_start" : null,
          other: null,
        })),
        noBlocks: args.blocks.length === 0,
        ok: args.blocks.length > 0 && args.blocks.every((b) => b.start !== ""),
      }),
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Ρυθμίσεις" }));
    await user.click(await screen.findByRole("button", { name: "Νέος εργαζόμενος" }));
    const dialog = screen.getByRole("dialog", { name: "Νέος εργαζόμενος" });

    // Saving with no hours at all is refused with a plain message.
    await user.click(within(dialog).getByRole("button", { name: "Αποθήκευση" }));
    expect(await within(dialog).findByText("Προσθέστε τουλάχιστον μία βάρδια.")).toBeInTheDocument();

    const monday = within(dialog).getByText("Δευτέρα").parentElement as HTMLElement;
    await user.click(within(monday).getByRole("button", { name: "+ Ώρες" }));
    const [start, end] = within(monday).getAllByRole("textbox");
    await user.type(end as HTMLElement, "2");
    await user.tab();
    expect(end).toHaveValue("02:00");
    expect(await within(monday).findByText("(+1)")).toBeInTheDocument();
    expect(within(monday).getByText("Γράψτε την έναρξη ως ΩΩ:ΛΛ, π.χ. 09:30.")).toBeInTheDocument();
    expect(start).toHaveAttribute("aria-invalid", "true");
  });
});

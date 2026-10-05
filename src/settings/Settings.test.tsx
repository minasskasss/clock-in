import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import App from "../App";
import { setLanguage } from "../i18n";
import { adminView, appState, callsTo, mockCommands } from "../test/tauri";

const noSession = { kind: "rejected", code: "no_session", retryAfterS: null, details: null };

describe("Settings", () => {
  beforeEach(async () => {
    await setLanguage("el");
  });

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

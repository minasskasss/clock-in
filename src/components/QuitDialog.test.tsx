import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import App, { QUIT_REQUESTED } from "../App";
import { emitTauriEvent } from "../test/setup";
import { appState, callsTo, mockCommands } from "../test/tauri";
import { QuitDialog } from "./QuitDialog";

const wrongCode = { kind: "invalid", field: "quit_code", problem: "wrong" };

describe("QuitDialog", () => {
  it("opens from Tray → Quit and sends the 4 digits tapped on the keypad", async () => {
    const calls = mockCommands({ app_state: appState(), quit: null });
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Μαρία");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await act(async () => emitTauriEvent(QUIT_REQUESTED));
    expect(await screen.findByRole("dialog", { name: "Έξοδος από το Clock In" })).toBeInTheDocument();
    for (const digit of ["1", "2", "3", "4"]) {
      await user.click(screen.getByRole("button", { name: digit }));
    }
    await waitFor(() => expect(callsTo(calls, "quit")).toEqual([{ code: "1234" }]));
  });

  it("keeps running after a wrong code, and starts the code again", async () => {
    const calls = mockCommands({
      quit: () => {
        throw wrongCode;
      },
    });
    const user = userEvent.setup();
    render(<QuitDialog codeSet onCancel={() => {}} />);
    for (const digit of ["9", "9", "9", "9"]) {
      await user.click(screen.getByRole("button", { name: digit }));
    }
    expect(await screen.findByText(/Λάθος κωδικός εξόδου/)).toBeInTheDocument();
    expect(callsTo(calls, "quit")).toEqual([{ code: "9999" }]);
    expect(screen.getByRole("group", { name: "Κωδικός εξόδου" })).toHaveAttribute("data-filled", "0");
    // A new attempt clears the message.
    await user.click(screen.getByRole("button", { name: "1" }));
    expect(screen.queryByText(/Λάθος κωδικός εξόδου/)).not.toBeInTheDocument();
  });

  it("accepts typed digits and backspace", async () => {
    const calls = mockCommands({ quit: null });
    const user = userEvent.setup();
    render(<QuitDialog codeSet onCancel={() => {}} />);
    await user.keyboard("12");
    await user.keyboard("{Backspace}");
    expect(screen.getByRole("group", { name: "Κωδικός εξόδου" })).toHaveAttribute("data-filled", "1");
    await user.click(screen.getByRole("button", { name: "Σβήσιμο τελευταίου ψηφίου" }));
    expect(screen.getByRole("group", { name: "Κωδικός εξόδου" })).toHaveAttribute("data-filled", "0");
    await user.keyboard("5678");
    await waitFor(() => expect(callsTo(calls, "quit")).toEqual([{ code: "5678" }]));
  });

  it("Cancel closes it without quitting", async () => {
    const calls = mockCommands({ app_state: appState() });
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Μαρία");
    await act(async () => emitTauriEvent(QUIT_REQUESTED));
    await user.click(await screen.findByRole("button", { name: "Άκυρο" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(callsTo(calls, "quit")).toEqual([]);
  });

  it("only confirms before a quit code exists", async () => {
    const calls = mockCommands({ quit: null });
    const user = userEvent.setup();
    render(<QuitDialog codeSet={false} onCancel={() => {}} />);
    expect(screen.getByText(/Δεν έχει οριστεί ακόμα κωδικός εξόδου/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "1" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Έξοδος" }));
    expect(callsTo(calls, "quit")).toEqual([{ code: "" }]);
  });
});

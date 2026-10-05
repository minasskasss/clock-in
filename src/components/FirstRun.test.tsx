import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLanguage } from "../i18n";
import { callsTo, mockCommands } from "../test/tauri";
import { FirstRun } from "./FirstRun";

const GOOD = "abacus abdomen abdominal abide abiding";

describe("First run", () => {
  beforeEach(async () => {
    await setLanguage("el");
  });

  it("sets up the first device only with a valid, repeated passphrase and quit code", async () => {
    const calls = mockCommands({
      check_new_passphrase: (args: { passphrase: string }) => {
        if (args.passphrase !== GOOD) {
          throw { kind: "invalid", field: "passphrase", problem: "unknown_words", positions: [1] };
        }
      },
      initialize: null,
    });
    const onDone = vi.fn();
    const user = userEvent.setup();
    render(<FirstRun defaultDeviceName="SHOP-PC" lockoutRemainingS={0} onDone={onDone} />);
    await user.click(screen.getByRole("button", { name: /Ρύθμιση ως πρώτη συσκευή/ }));

    const submit = screen.getByRole("button", { name: "Ολοκλήρωση ρύθμισης" });
    await user.type(screen.getByLabelText("Κωδικός φράση"), "abacus wrongword x y z");
    expect(await screen.findByText(/Δεν είναι στη λίστα: λέξη 2/)).toBeInTheDocument();

    await user.clear(screen.getByLabelText("Κωδικός φράση"));
    await user.type(screen.getByLabelText("Κωδικός φράση"), GOOD);
    expect(await screen.findByText(/Ο κωδικός φράση είναι έγκυρος/)).toBeInTheDocument();
    await user.type(screen.getByLabelText("Ξανά ο κωδικός φράση"), "abacus");
    expect(screen.getByText("Οι δύο κωδικοί φράσεις δεν είναι ίδιοι.")).toBeInTheDocument();
    await user.clear(screen.getByLabelText("Ξανά ο κωδικός φράση"));
    await user.type(screen.getByLabelText("Ξανά ο κωδικός φράση"), GOOD);

    await user.type(screen.getByLabelText("Κωδικός εξόδου"), "12a");
    expect(screen.getByLabelText("Κωδικός εξόδου")).toHaveValue("12");
    expect(screen.getByText("Ακριβώς 4 ψηφία.")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Κωδικός εξόδου"), "34");
    await user.type(screen.getByLabelText("Ξανά ο κωδικός εξόδου"), "1234");
    expect(screen.getByLabelText("Όνομα συσκευής")).toHaveValue("SHOP-PC");

    expect(submit).toBeEnabled();
    await user.click(submit);
    expect(callsTo(calls, "initialize")).toEqual([{ passphrase: GOOD, quitCode: "1234", deviceName: "SHOP-PC" }]);
    expect(onDone).toHaveBeenCalled();
  });

  it("explains when a first device already exists", async () => {
    mockCommands({
      check_new_passphrase: null,
      initialize: () => {
        throw { kind: "rejected", code: "already_initialized", retryAfterS: null, details: null };
      },
    });
    const user = userEvent.setup();
    render(<FirstRun defaultDeviceName="PC" lockoutRemainingS={0} onDone={() => {}} />);
    await user.click(screen.getByRole("button", { name: /Ρύθμιση ως πρώτη συσκευή/ }));
    await user.type(screen.getByLabelText("Κωδικός φράση"), GOOD);
    await user.type(screen.getByLabelText("Ξανά ο κωδικός φράση"), GOOD);
    await user.type(screen.getByLabelText("Κωδικός εξόδου"), "1234");
    await user.type(screen.getByLabelText("Ξανά ο κωδικός εξόδου"), "1234");
    await screen.findByText(/Ο κωδικός φράση είναι έγκυρος/);
    await user.click(screen.getByRole("button", { name: "Ολοκλήρωση ρύθμισης" }));
    expect(await screen.findByText(/Υπάρχει ήδη πρώτη συσκευή/)).toBeInTheDocument();
  });

  it("pairs with the passphrase, and shows the lockout instead of the form", async () => {
    const calls = mockCommands({ pair: null });
    const user = userEvent.setup();
    const { rerender } = render(<FirstRun defaultDeviceName="PC (b)" lockoutRemainingS={0} onDone={() => {}} />);
    await user.click(screen.getByRole("button", { name: /Σύνδεση αυτής της συσκευής/ }));
    await user.type(screen.getByLabelText("Κωδικός φράση"), GOOD);
    await user.click(screen.getByRole("button", { name: "Σύνδεση" }));
    expect(callsTo(calls, "pair")).toEqual([{ passphrase: GOOD, deviceName: "PC (b)" }]);

    rerender(<FirstRun defaultDeviceName="PC (b)" lockoutRemainingS={60} onDone={() => {}} />);
    expect(screen.getByText("Πολλές λάθος προσπάθειες. Δοκιμάστε ξανά σε 1:00.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Σύνδεση" })).toBeDisabled();
  });
});

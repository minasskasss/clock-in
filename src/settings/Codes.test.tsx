import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import App from "../App";
import { adminView, appState, callsTo, mockCommands } from "../test/tauri";

const CURRENT = "abacus abdomen abdominal abide abiding";
const GENERATED = "cactus cadet cage cake calamity";

const normalize = (s: string) => s.trim().toLowerCase().split(/\s+/).join(" ");

/** Rust's check, as far as these tests need it. */
function checkNewPassphrase(args: { passphrase: string; current: string | null }) {
  if (args.current !== null && normalize(args.current) === normalize(args.passphrase)) {
    throw { kind: "invalid", field: "passphrase", problem: "same_as_current" };
  }
}

async function openCodes(handlers: Record<string, unknown>) {
  const calls = mockCommands({
    app_state: appState({ adminUnlocked: true }),
    admin_view: adminView(),
    check_new_passphrase: checkNewPassphrase,
    generate_passphrase: GENERATED,
    change_passphrase: null,
    ...handlers,
  });
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole("button", { name: "Ρυθμίσεις" }));
  await user.click(await screen.findByRole("tab", { name: "Κωδικοί" }));
  return { calls, user };
}

/** The box and its Show/Hide button. */
function passphraseField(label: string) {
  const box = screen.getByLabelText(label);
  const toggle = within(box.parentElement as HTMLElement).getByRole("button");
  return { box, toggle };
}

describe("Codes", () => {
  it("Δημιουργία fills only the new box, shown, and Show/Hide works in every box", async () => {
    const { user } = await openCodes({});
    const current = passphraseField("Τωρινός κωδικός φράση");
    const next = passphraseField("Νέος κωδικός φράση");
    const repeat = passphraseField("Ξανά ο νέος κωδικός φράση");

    // Before Generate: all hidden, each toggles on its own.
    for (const field of [current, next, repeat]) {
      expect(field.box).toHaveAttribute("type", "password");
      await user.click(field.toggle);
      expect(field.box).toHaveAttribute("type", "text");
      expect(field.toggle).toHaveAccessibleName("Απόκρυψη κωδικού");
      await user.click(field.toggle);
      expect(field.box).toHaveAttribute("type", "password");
      expect(field.toggle).toHaveAccessibleName("Εμφάνιση κωδικού");
    }

    await user.click(screen.getByRole("button", { name: "Δημιουργία" }));
    expect(next.box).toHaveValue(GENERATED);
    expect(next.box).toHaveAttribute("type", "text");
    expect(repeat.box).toHaveValue("");
    expect(repeat.box).toHaveAttribute("type", "password");

    // The generated one can be hidden and shown again.
    await user.click(next.toggle);
    expect(next.box).toHaveAttribute("type", "password");
    await user.click(next.toggle);
    expect(next.box).toHaveAttribute("type", "text");

    // Hidden, then Generate again: shown again for writing down.
    await user.click(next.toggle);
    await user.click(screen.getByRole("button", { name: "Δημιουργία" }));
    expect(next.box).toHaveAttribute("type", "text");

    // The repeat box still toggles after Generate, and so does the current one.
    await user.click(repeat.toggle);
    expect(repeat.box).toHaveAttribute("type", "text");
    await user.click(repeat.toggle);
    expect(repeat.box).toHaveAttribute("type", "password");
    await user.click(current.toggle);
    expect(current.box).toHaveAttribute("type", "text");
  });

  it("after Δημιουργία, the second box must be typed before the change can be sent", async () => {
    const { calls, user } = await openCodes({});
    const submit = screen.getByRole("button", { name: "Αλλαγή κωδικού φράσης" });
    await user.type(screen.getByLabelText("Τωρινός κωδικός φράση"), CURRENT);
    await user.click(screen.getByRole("button", { name: "Δημιουργία" }));
    expect(submit).toBeDisabled();

    await user.type(screen.getByLabelText("Ξανά ο νέος κωδικός φράση"), "cactus cadet cage cake calamitx");
    expect(screen.getByText("Οι δύο νέοι κωδικοί φράσεις δεν είναι ίδιοι.")).toBeInTheDocument();
    expect(submit).toBeDisabled();

    await user.type(screen.getByLabelText("Ξανά ο νέος κωδικός φράση"), "{Backspace}y");
    expect(submit).toBeEnabled();
    await user.click(submit);
    expect(callsTo(calls, "change_passphrase")).toEqual([{ current: CURRENT, newPassphrase: GENERATED }]);
  });

  it("refuses a new passphrase equal to the current one, after normalisation", async () => {
    const { calls, user } = await openCodes({});
    const submit = screen.getByRole("button", { name: "Αλλαγή κωδικού φράσης" });
    await user.type(screen.getByLabelText("Τωρινός κωδικός φράση"), CURRENT);
    await user.type(screen.getByLabelText("Νέος κωδικός φράση"), "  Abacus ABDOMEN abdominal  abide abiding");
    await user.type(screen.getByLabelText("Ξανά ο νέος κωδικός φράση"), "  Abacus ABDOMEN abdominal  abide abiding");
    expect(
      await screen.findByText("Ο νέος κωδικός φράση είναι ίδιος με τον τωρινό. Διαλέξτε άλλον."),
    ).toBeInTheDocument();
    expect(submit).toBeDisabled();
    expect(callsTo(calls, "check_new_passphrase")).toContainEqual({
      passphrase: "  Abacus ABDOMEN abdominal  abide abiding",
      current: CURRENT,
    });
    expect(callsTo(calls, "change_passphrase")).toHaveLength(0);
  });

  it("refuses a new quit code equal to the current one with a clear message", async () => {
    const { calls, user } = await openCodes({
      settings_save: (args: { quitCode: string }) => {
        if (args.quitCode === "1234") throw { kind: "invalid", field: "quit_code", problem: "same_as_current" };
      },
    });
    await user.type(screen.getByLabelText("Νέος κωδικός εξόδου"), "1234");
    await user.type(screen.getByLabelText("Ξανά ο νέος κωδικός εξόδου"), "1234");
    await user.click(screen.getByRole("button", { name: "Αλλαγή κωδικού εξόδου" }));
    expect(
      await screen.findByText("Ο νέος κωδικός εξόδου είναι ίδιος με τον τωρινό. Διαλέξτε άλλον."),
    ).toBeInTheDocument();
    expect(screen.queryByText("Ο κωδικός εξόδου άλλαξε.")).not.toBeInTheDocument();
    expect(callsTo(calls, "settings_save")).toHaveLength(1);
  });
});

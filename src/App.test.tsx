import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import App from "./App";
import { setLanguage } from "./i18n";
import { setSystemPrefersDark } from "./test/setup";

describe("App", () => {
  beforeEach(async () => {
    await setLanguage("el");
  });

  it("shows the Greek placeholder by default, with the background", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "Σύντομα εδώ: το σημερινό πρόγραμμα" })).toBeInTheDocument();
    expect(screen.getByText("Ήρθε")).toBeInTheDocument();
    expect(screen.getByTestId("background")).toHaveAttribute("aria-hidden", "true");
  });

  it("switches the placeholder text to English from the menu and back", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole("button", { name: "Μενού" }));
    await user.click(screen.getByRole("button", { name: "English" }));
    expect(screen.getByRole("heading", { name: "Coming soon: today's schedule" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "English" })).toHaveAttribute("aria-pressed", "true");

    await user.click(screen.getByRole("button", { name: "Ελληνικά" }));
    expect(screen.getByRole("heading", { name: "Σύντομα εδώ: το σημερινό πρόγραμμα" })).toBeInTheDocument();
  });

  it("switches between light and dark from the menu", async () => {
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
    render(<App />);
    expect(document.documentElement.dataset.theme).toBe("light");
    act(() => setSystemPrefersDark(true));
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("closes the menu with Escape", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: "Μενού" }));
    expect(screen.getByRole("button", { name: "Σκοτεινό" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("button", { name: "Σκοτεινό" })).not.toBeInTheDocument();
  });
});

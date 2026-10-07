import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import App from "../App";
import type { DiagnosticsView } from "../api";
import { appState, mockCommands } from "../test/tauri";

function diagnostics(): DiagnosticsView {
  return {
    appVersion: "0.5.0",
    environment: "prod",
    phone: "Xiaomi Redmi Note 11",
    androidVersion: "13",
    sdk: 33,
    makerOs: "HyperOS OS1.0.8.0.TGCMIXM",
    alertMode: "ring",
    permissions: {
      notifications: true,
      exactAlarms: true,
      fullScreen: false,
      battery: true,
      unusedApps: true,
      oem: "xiaomi",
      oemDone: true,
    },
    lastSync: { date: "2026-10-07", time: "08:00" },
    lastRefresh: { date: "2026-10-07", time: "08:15" },
    lastRefreshOk: true,
    nextAlarm: { date: "2026-10-07", time: "12:00" },
    lastAlarm: { date: "2026-10-06", time: "21:30" },
    lastAlarmHow: "notification",
  };
}

describe("Diagnostics", () => {
  it("opens from the menu on Android and shows the phone, settings and alarms in plain Greek", async () => {
    window.localStorage.setItem("clockin.onboarding", "done");
    mockCommands({
      app_state: appState({
        platform: "android",
        android: { alertMode: "ring", permissions: null, permissionsOk: true, alarm: null },
      }),
      android_diagnostics: diagnostics(),
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Μενού" }));
    await user.click(screen.getByRole("button", { name: "Διαγνωστικά" }));
    expect(await screen.findByText("Xiaomi Redmi Note 11")).toBeInTheDocument();
    expect(screen.getByText("Clock In 0.5.0 (κανονική)")).toBeInTheDocument();
    expect(screen.getByText("13 (API 33)")).toBeInTheDocument();
    expect(screen.getByText("HyperOS OS1.0.8.0.TGCMIXM")).toBeInTheDocument();
    expect(screen.getByText("Ειδοποιήσεις σε πλήρη οθόνη").nextSibling).toHaveTextContent("✗");
    expect(screen.getByText("07/10/2026 08:15, επιτυχής")).toBeInTheDocument();
    expect(screen.getByText("07/10/2026 12:00")).toBeInTheDocument();
    expect(screen.getByText("06/10/2026 21:30, μόνο ως ειδοποίηση, χωρίς πλήρη οθόνη")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Πίσω" }));
    expect(screen.queryByText("Xiaomi Redmi Note 11")).not.toBeInTheDocument();
    window.localStorage.removeItem("clockin.onboarding");
  });

  it("is not in the Windows menu", async () => {
    mockCommands({ app_state: appState() });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Μενού" }));
    expect(screen.queryByRole("button", { name: "Διαγνωστικά" })).not.toBeInTheDocument();
  });
});

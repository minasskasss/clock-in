import { invoke } from "@tauri-apps/api/core";
import { vi } from "vitest";
import type { AdminView, AppStateView, Row, TodayView } from "../api";

type Args = Record<string, unknown> | undefined;
type Handler = (args: Args) => unknown;

export interface Call {
  command: string;
  args: Args;
}

/**
 * Answers Rust commands from `handlers` (a value, or a function of the
 * arguments; a thrown value becomes the command's error). Returns the log
 * of calls.
 */
export function mockCommands(handlers: Record<string, unknown>): Call[] {
  const calls: Call[] = [];
  vi.mocked(invoke).mockImplementation(async (command, rawArgs) => {
    const args = rawArgs as Args;
    calls.push({ command, args });
    if (!(command in handlers)) return undefined;
    const handler = handlers[command];
    return typeof handler === "function" ? await (handler as Handler)(args) : handler;
  });
  return calls;
}

export function callsTo(calls: Call[], command: string): Args[] {
  return calls.filter((c) => c.command === command).map((c) => c.args);
}

export function row(overrides: Partial<Row> = {}): Row {
  return {
    key: "b1:2026-10-05",
    staffId: "s1",
    sourceBlockId: "b1",
    businessDate: "2026-10-05",
    firstName: "Μαρία",
    lastName: "Παππά",
    start: "12:00",
    end: "16:00",
    startNextDay: false,
    endNextDay: false,
    status: "pending",
    nextMark: "in",
    ...overrides,
  };
}

export function today(rows: Row[] = [row()]): TodayView {
  return { businessDate: "2026-10-05", clock: "11:42", rows };
}

export function appState(overrides: Partial<AppStateView> = {}): AppStateView {
  return {
    phase: "paired",
    environment: "dev",
    debug: null,
    lockoutRemainingS: 0,
    defaultDeviceName: "SHOP-PC",
    today: today(),
    banners: { offline: false, lastSync: null, clockSkew: false, horizonShort: false },
    adminUnlocked: false,
    dataVersion: 1,
    ...overrides,
  };
}

export function adminView(overrides: Partial<AdminView> = {}): AdminView {
  return {
    today: "2026-10-05",
    staff: [
      {
        id: "s1",
        firstName: "Μαρία",
        lastName: "Παππά",
        blocks: [
          { id: "b1", weekday: 1, start: "12:00", end: "16:00", startNextDay: false, endNextDay: false },
          { id: "b2", weekday: 1, start: "19:00", end: "02:00", startNextDay: false, endNextDay: true },
        ],
      },
    ],
    overrides: [],
    settings: { checkinOffsetMin: 0, checkoutOffsetMin: 0, rollover: "05:00", autostart: true },
    marks: [],
    devices: [
      {
        id: "d1",
        name: "SHOP-PC",
        platform: "windows",
        pairedAt: { date: "2026-10-01", time: "10:00" },
        lastSeen: { date: "2026-10-05", time: "11:41" },
        thisDevice: true,
      },
    ],
    ...overrides,
  };
}

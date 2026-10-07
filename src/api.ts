/**
 * Typed wrappers over the Rust commands (src-tauri/src/commands.rs).
 * The UI never talks to the server itself and never does time arithmetic:
 * every time it shows comes from Rust as a Greek wall-clock string.
 */
import { invoke } from "@tauri-apps/api/core";

export type Phase = "not_configured" | "unpaired" | "paired";
export type RowStatus = "pending" | "late" | "checked_in" | "should_have_left" | "left";
export type MarkKind = "in" | "out";
export type OverrideKind = "off" | "replace";
export type Platform = "windows" | "android";

export interface LocalStamp {
  /** ISO date, e.g. "2026-10-05". */
  date: string;
  /** "HH:MM". */
  time: string;
}

export interface Row {
  key: string;
  staffId: string;
  sourceBlockId: string;
  businessDate: string;
  firstName: string;
  lastName: string;
  start: string;
  end: string;
  startNextDay: boolean;
  endNextDay: boolean;
  status: RowStatus;
  nextMark: MarkKind | null;
}

export interface TodayView {
  businessDate: string;
  clock: string;
  rows: Row[];
}

export interface Banners {
  offline: boolean;
  lastSync: LocalStamp | null;
  clockSkew: boolean;
  horizonShort: boolean;
  /** Windows sound output muted, at zero or missing. */
  soundOff: boolean;
  /** Marks the server refused (not saved), oldest first, until dismissed. */
  refusedMarks: RefusedMark[];
}

export interface RefusedMark {
  id: string;
  /** Empty if this device didn't know the person. */
  firstName: string;
  lastName: string;
  kind: MarkKind;
  /** ISO business date, only if it isn't the day shown on Today. */
  otherDate: string | null;
}

export interface AppStateView {
  phase: Phase;
  environment: "dev" | "prod";
  debug: { profile: string | null; fakeClock: string | null; fakeClockSecond: boolean } | null;
  lockoutRemainingS: number;
  defaultDeviceName: string;
  today: TodayView | null;
  banners: Banners;
  adminUnlocked: boolean;
  dataVersion: number;
  /** Whether the automatic theme is dark right now (decided in Rust). */
  autoDark: boolean;
  /** Whether Quit asks for the quit code (false before one is known). */
  quitCodeSet: boolean;
  platform: Platform;
  /** Android only. */
  android: AndroidView | null;
}

export type AlertMode = "ring" | "notification";

/** One Android checklist item (src-tauri: `android_open_settings`). */
export type PermissionKind =
  | "notifications"
  | "exactAlarms"
  | "fullScreen"
  | "battery"
  | "unusedApps"
  | "oem"
  | "xiaomiAutostart"
  | "xiaomiPermissions"
  | "xiaomiBattery";

/** The Android onboarding checklist (SPEC §8.2). */
export interface PermissionStatus {
  notifications: boolean;
  exactAlarms: boolean;
  fullScreen: boolean;
  battery: boolean;
  /** "Pause app activity if unused" is off. */
  unusedApps: boolean;
  /** The phone maker if it needs an extra step ("samsung", "xiaomi", …), or "". */
  oem: string;
  /** The user said the maker step is done (it can't be checked). */
  oemDone: boolean;
}

export interface AndroidView {
  alertMode: AlertMode;
  /** Null until Android was first asked. */
  permissions: PermissionStatus | null;
  /** Everything the alarms need is granted (the maker step is advice). */
  permissionsOk: boolean;
  /** A ring-mode alarm in progress on this phone (Today shows «Σταμάτημα»). */
  alarm: AlarmBanner | null;
}

export interface AlarmBanner {
  /** False during the silent minutes between rings. */
  ringing: boolean;
  checkIn: string[];
  checkOut: string[];
}

/** «Διαγνωστικά» (Android): read-only, no secrets. */
export interface DiagnosticsView {
  appVersion: string;
  environment: "dev" | "prod";
  phone: string;
  androidVersion: string;
  sdk: number;
  /** MIUI, HyperOS or One UI version, or "". */
  makerOs: string;
  alertMode: AlertMode;
  permissions: PermissionStatus | null;
  lastSync: LocalStamp | null;
  lastRefresh: LocalStamp | null;
  lastRefreshOk: boolean | null;
  nextAlarm: LocalStamp | null;
  lastAlarm: LocalStamp | null;
  lastAlarmHow: "fullScreen" | "opened" | "notification" | "notificationMode" | null;
}

/** The alarm window's content (src-tauri/src/alarms.rs). Times are "HH:MM". */
export interface AlarmView {
  id: number;
  at: string;
  checkIn: string[];
  checkOut: string[];
  /** False during the silent part of the cycle. */
  ringing: boolean;
  reringAt: string | null;
}

export interface AlarmStateView {
  /** Null once the alarm ended (the window is about to close). */
  alarm: AlarmView | null;
  autoDark: boolean;
}

export interface BlockView {
  id: string;
  start: string;
  end: string;
  startNextDay: boolean;
  endNextDay: boolean;
}

export interface WeekBlockView extends BlockView {
  /** 1 = Monday … 7 = Sunday. */
  weekday: number;
}

export interface StaffView {
  id: string;
  firstName: string;
  lastName: string;
  blocks: WeekBlockView[];
}

export interface OverrideView {
  id: string;
  staffId: string;
  firstName: string;
  lastName: string;
  businessDate: string;
  kind: OverrideKind;
  blocks: BlockView[];
}

export interface SettingsView {
  checkinOffsetMin: number;
  checkoutOffsetMin: number;
  rollover: string;
  autostart: boolean;
}

export interface MarkView {
  id: string;
  firstName: string;
  lastName: string;
  kind: MarkKind;
  markedAt: LocalStamp;
  block: BlockView | null;
}

export interface DeviceView {
  id: string;
  name: string;
  platform: Platform;
  pairedAt: LocalStamp;
  lastSeen: LocalStamp | null;
  thisDevice: boolean;
}

export interface AdminView {
  today: string;
  staff: StaffView[];
  overrides: OverrideView[];
  settings: SettingsView;
  marks: MarkView[];
  devices: DeviceView[];
}

export interface WeekBlockDraft {
  id?: string | null;
  weekday: number;
  start: string;
  end: string;
}

export interface RangeDraft {
  id?: string | null;
  start: string;
  end: string;
}

export type BlockProblem =
  | "bad_start"
  | "bad_end"
  | "bad_weekday"
  | "start_equals_end"
  | "too_short"
  | "too_long"
  | "overlaps";

export interface BlockReport {
  startNextDay: boolean;
  endNextDay: boolean;
  problem: BlockProblem | null;
  other: number | null;
}

export interface WeekReport {
  blocks: BlockReport[];
  noBlocks: boolean;
  ok: boolean;
}

export interface OverrideReport {
  dateProblem: "bad_date" | "past_date" | null;
  blocks: BlockReport[];
  noBlocks: boolean;
  ok: boolean;
}

/** What a failed command returns (src-tauri/src/error.rs). */
export type CmdError =
  | { kind: "not_configured" }
  | { kind: "not_paired" }
  | { kind: "offline" }
  | { kind: "rejected"; code: string; retryAfterS: number | null; details: string | null }
  | { kind: "invalid"; field: string; problem: string; positions?: number[]; character?: string }
  | { kind: "internal"; message: string };

export function isCmdError(value: unknown): value is CmdError {
  return typeof value === "object" && value !== null && "kind" in value;
}

/** Normalises anything a command threw into a CmdError. */
export function toCmdError(value: unknown): CmdError {
  if (isCmdError(value)) return value;
  return { kind: "internal", message: String(value) };
}

export function isNoSession(error: unknown): boolean {
  const e = toCmdError(error);
  return e.kind === "rejected" && e.code === "no_session";
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toCmdError(error);
  }
}

export const api = {
  appState: () => call<AppStateView>("app_state"),
  /** `current` (typed in the same form): refuses a new passphrase equal to it. */
  checkNewPassphrase: (passphrase: string, current?: string) =>
    call<void>("check_new_passphrase", { passphrase, current: current ?? null }),
  initialize: (passphrase: string, quitCode: string, deviceName: string) =>
    call<void>("initialize", { passphrase, quitCode, deviceName }),
  pair: (passphrase: string, deviceName: string) => call<void>("pair", { passphrase, deviceName }),
  mark: (row: Row, kind: MarkKind) =>
    call<void>("mark", {
      staffId: row.staffId,
      sourceBlockId: row.sourceBlockId,
      businessDate: row.businessDate,
      kind,
    }),
  dismissRefusedMark: (id: string) => call<void>("dismiss_refused_mark", { id }),
  adminLogin: (passphrase: string) => call<void>("admin_login", { passphrase }),
  adminLogout: () => call<void>("admin_logout"),
  adminTouch: () => call<void>("admin_touch"),
  adminView: () => call<AdminView | null>("admin_view"),
  validateWeek: (blocks: WeekBlockDraft[]) => call<WeekReport>("validate_week", { blocks }),
  validateOverride: (kind: OverrideKind, businessDate: string, blocks: RangeDraft[]) =>
    call<OverrideReport>("validate_override", { kind, businessDate, blocks }),
  staffSave: (id: string | null, firstName: string, lastName: string, blocks: WeekBlockDraft[]) =>
    call<void>("staff_save", { id, firstName, lastName, blocks }),
  staffRemove: (id: string) => call<void>("staff_remove", { id }),
  overrideSave: (staffId: string, businessDate: string, kind: OverrideKind, blocks: RangeDraft[]) =>
    call<void>("override_save", { staffId, businessDate, kind, blocks }),
  overrideDelete: (id: string) => call<void>("override_delete", { id }),
  settingsSave: (s: {
    checkinOffsetMin: number;
    checkoutOffsetMin: number;
    rollover: string;
    autostart: boolean;
    quitCode: string | null;
  }) => call<void>("settings_save", s),
  markVoid: (id: string) => call<void>("mark_void", { id }),
  deviceRevoke: (id: string) => call<void>("device_revoke", { id }),
  changePassphrase: (current: string, newPassphrase: string) =>
    call<void>("change_passphrase", { current, newPassphrase }),
  generatePassphrase: () => call<string>("generate_passphrase"),
  alarmState: () => call<AlarmStateView>("alarm_state"),
  alarmStop: (id: number) => call<void>("alarm_stop", { id }),
  /** Exits the app if `code` is the quit code; otherwise rejects. */
  quit: (code: string) => call<void>("quit", { code }),
  debugSetClock: (local: string | null, second = false) => call<void>("debug_set_clock", { local, second }),
  /** Android: ring or notification, per device. */
  setAlertMode: (mode: AlertMode) => call<void>("set_alert_mode", { mode }),
  /** Android: opens the phone's screen that fixes one checklist item. */
  androidOpenSettings: (kind: PermissionKind) => call<void>("android_open_settings", { kind }),
  androidSetOemDone: (done: boolean) => call<void>("android_set_oem_done", { done }),
  androidStopAlarm: () => call<void>("android_stop_alarm"),
  androidDiagnostics: () => call<DiagnosticsView | null>("android_diagnostics"),
  setTheme: (theme: "auto" | "system" | "light" | "dark") => call<void>("set_theme", { theme }),
};

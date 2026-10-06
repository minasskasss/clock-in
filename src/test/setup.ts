import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";
import "../i18n";

// No Tauri in tests: every command goes through a mock (see `mockCommands`).
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

// Events from Rust (e.g. Tray → Quit): tests send them with `emitTauriEvent`.
type EventHandler = (event: { payload: unknown }) => void;
const tauriEvents = vi.hoisted(() => new Map<string, Set<EventHandler>>());
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: EventHandler) => {
    const handlers = tauriEvents.get(name) ?? new Set<EventHandler>();
    handlers.add(handler);
    tauriEvents.set(name, handlers);
    return () => handlers.delete(handler);
  }),
}));

export function emitTauriEvent(name: string, payload: unknown = null): void {
  tauriEvents.get(name)?.forEach((handler) => handler({ payload }));
}

/** jsdom has no matchMedia. Tests flip `systemPrefersDark` to simulate the OS setting. */
export const mediaState = { systemPrefersDark: false };
const listeners = new Set<() => void>();

export function setSystemPrefersDark(dark: boolean): void {
  mediaState.systemPrefersDark = dark;
  listeners.forEach((listener) => listener());
}

vi.stubGlobal("matchMedia", (query: string): MediaQueryList => {
  const isDarkQuery = query === "(prefers-color-scheme: dark)";
  return {
    get matches() {
      return isDarkQuery && mediaState.systemPrefersDark;
    },
    media: query,
    onchange: null,
    addEventListener: (_type: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_type: string, listener: () => void) => listeners.delete(listener),
    addListener: (listener: () => void) => listeners.add(listener),
    removeListener: (listener: () => void) => listeners.delete(listener),
    dispatchEvent: () => false,
  } as unknown as MediaQueryList;
});

beforeEach(() => {
  window.localStorage.clear();
  mediaState.systemPrefersDark = false;
  delete document.documentElement.dataset.theme;
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

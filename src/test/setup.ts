import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";

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
});

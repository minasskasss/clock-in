import { useCallback, useEffect, useState } from "react";
import { api, type AppStateView } from "./api";

/** How often the main screen asks Rust for the current state (clock, rows, banners). */
export const POLL_MS = 1000;

/**
 * The app state from Rust, refreshed every second and on demand. Polling
 * keeps the clock and the row statuses current without any time logic here.
 */
export function useAppState(): { state: AppStateView | null; failed: boolean; refresh: () => Promise<void> } {
  const [state, setState] = useState<AppStateView | null>(null);
  const [failed, setFailed] = useState(false);

  const refresh = useCallback(
    () =>
      api.appState().then(
        (next) => {
          setState(next);
          setFailed(false);
        },
        () => setFailed(true),
      ),
    [],
  );

  useEffect(() => {
    let alive = true;
    const load = () =>
      api.appState().then(
        (next) => {
          if (!alive) return;
          setState(next);
          setFailed(false);
        },
        () => alive && setFailed(true),
      );
    void load();
    const timer = window.setInterval(() => void load(), POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  return { state, failed, refresh };
}

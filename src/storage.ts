/**
 * Per-device UI preferences (theme). These need no passphrase and
 * are not synced. localStorage can throw (e.g. storage disabled), so every
 * access is guarded and falls back to the default.
 */
export function readPreference(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writePreference(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Not fatal: the preference simply isn't remembered.
  }
}

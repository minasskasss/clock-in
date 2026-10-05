import { createContext, useContext } from "react";
import type { AdminView } from "../api";

export interface Admin {
  /** The Settings lists, from the last snapshot. */
  view: AdminView;
  /**
   * Runs an admin change. If the server session ran out meanwhile, asks for
   * the passphrase in place and tries once more, so an open form survives.
   */
  run: <T>(action: () => Promise<T>) => Promise<T>;
  /** Reloads the lists. */
  reload: () => Promise<void>;
}

export const AdminContext = createContext<Admin | null>(null);

export function useAdmin(): Admin {
  const admin = useContext(AdminContext);
  if (!admin) throw new Error("useAdmin outside Settings");
  return admin;
}

# Decision log

Format: `date | decision | reason`. One line each. Newest at the bottom.

## From planning (Minas + Claude chat, 2026-10-04)

2026-10-04 | Devices: shop PC (Windows 10, Rhodes) + employer's Android phone only; no staff phones | Owner decision
2026-10-04 | Every device alerts for everyone; no per-device staff filtering | Follows from the two-device setup
2026-10-04 | Shift patterns: split shifts, shifts past midnight, one-off date overrides | Owner decision
2026-10-04 | Stopping an alarm affects only the device it was pressed on | Owner decision
2026-10-04 | Stop does NOT mark anyone; marking is a separate tap on the row | Owner decision
2026-10-04 | Alarm offsets adjustable in passphrase-protected settings, default 0 (exactly at start/end) | Owner decision
2026-10-04 | Unanswered alarm: ring 5 min, silent 5 min, repeat until Stop | Owner decision
2026-10-04 | Re-rings drop names already marked; the cycle ends when none are left | Assumption accepted by owner
2026-10-04 | Android notification mode: single notification, no repeat | Assumption accepted by owner
2026-10-04 | Marking: confirmation dialog only, no undo | Owner decision
2026-10-04 | Mistaken marks are corrected by the employer in Settings → Today's marks | Keeps a correction path without a staff undo
2026-10-04 | Quitting the Windows app needs a 4-digit quit code (not the passphrase); not a security control | Owner decision: prevents accidental closing; staff never learn the passphrase
2026-10-04 | Quit code set at first run, changeable in Settings, synced, checked locally (works offline) | Follows from the above
2026-10-04 | Business-day rollover default 05:00 | Assumption accepted by owner
2026-10-04 | Greek UI default, English toggle | Assumption accepted by owner
2026-10-04 | Supabase (EU Frankfurt): tables in unexposed schema `app`, RPC + device secrets; no Supabase Auth/Realtime/Firebase | One auth path for Rust and Kotlin; tables unreachable
2026-10-04 | Migrations via `supabase db push --db-url` with session-pooler connection strings in env vars | No CLI login or access token needed; IPv4-compatible
2026-10-04 | 5-word EFF passphrase, bcrypt cost 12, lockout doubling to 60 min | Owner's security requirement; ~64.6 bits of entropy
2026-10-04 | The passphrase and quit code are never handled by Claude | Security
2026-10-04 | 30-day retention for marks and overrides | Data minimisation (GDPR)
2026-10-04 | No auto-updater in v1; remote installs via AnyDesk (PC) and APK file (phone) | Two devices; owner is in Athens, devices in Rhodes
2026-10-04 | Local sessions: Claude desktop app, Code tab, Local, Auto mode, no worktree, `C:\dev\clock-in` | Owner's setup
2026-10-04 | Phases 0–1 in Claude Code cloud sessions; Phases 2–6 local | Cloud VM is Linux: it can't run or package the Windows app, can't use Android devices, very likely can't open Postgres connections through its HTTP proxy, and DB secrets must stay off cloud environments
2026-10-04 | Repo `clock-in` created by Minas, with the docs pushed to `main`; Claude Code never creates or re-initialises it | Owner decision
2026-10-04 | GitHub "Automatically delete head branches" is on; cloud sessions merge without deleting branches | The cloud GitHub proxy rejects branch deletions
2026-10-04 | CI has Linux and Windows jobs; the Windows job decides on Windows-only code | Cloud sessions compile on Linux only
2026-10-04 | Claude Code merges only after Minas writes "approved, merge" | Owner reviews every phase
2026-10-04 | Android signing keystore lives in `C:\dev\clock-in-keys\`, outside the repo, backed up twice | Losing it breaks in-place updates
2026-10-04 | Android developer verification (global in 2027) to be handled before then via Android Developer Console | Sideloading rules for certified devices

## Phase 0 — Scaffold (cloud session, 2026-10-04)

2026-10-04 | Cloud tool versions: Ubuntu 24.04.4, git 2.43.0, gh 2.89.0, rustc/cargo 1.97.0, rustup 1.28.2, Node 22.22.0, pnpm 10.28.0; Tauri Linux deps present from the setup script | `check-tools` record; nothing had to be installed in-session
2026-10-04 | Phase 0 branch is the session's assigned branch `claude/jolly-goodall-2h8wmz`, not `phase-0-scaffold` | The cloud session may push only to its assigned branch; the stop report gives the exact name for testing
2026-10-04 | In cloud sessions, PRs and CI are handled through the GitHub connector instead of `gh` | `gh` has no valid token in the cloud VM
2026-10-04 | EFF large wordlist from https://www.eff.org/files/2016/07/18/eff_large_wordlist.txt, 7776 lines, SHA-256 `addd35536511597a02fa0a9ff1e5284677b8883b83e986e43f15a3db996b903e` | Integrity record (PLAN Phase 0); matches the published list
2026-10-04 | Stack versions: Tauri 2.12 (crate and CLI), React 19.3, Vite 8.3, TypeScript 6.0, Vitest 5, ESLint 10 (flat config) with typescript-eslint 8 | Current stable; TypeScript 7 is skipped because typescript-eslint supports only < 6.1
2026-10-04 | `jiff` with `tzdb-bundle-always` (no system tz database) | Windows and Android have no usable zoneinfo; one bundled database gives identical Europe/Athens rules on every platform and in tests
2026-10-04 | App identifier / Android package name `io.github.minasskasss.clockin` | Owner-namespaced and unique; it must never change after the first APK, or updates stop installing over the old app
2026-10-04 | Rust edition 2024, workspace resolver 3, `unsafe_code = "deny"` workspace-wide | Modern defaults; Win32 code (Phase 4) opts in locally with `#[allow(unsafe_code)]` and a SAFETY comment
2026-10-04 | Bundle target NSIS only | ARCHITECTURE §13 ships an NSIS installer; skipping MSI keeps builds faster
2026-10-04 | Strict CSP: `'self'` scripts, inline styles allowed, `data:` images, IPC only for connect | No network from the webview (all Supabase calls go through Rust); checked the built page in Chromium with no violations
2026-10-04 | Language and theme are stored per device in webview localStorage | Per-device, passphrase-free preferences (SPEC §3); may move into the local store when it exists (Phase 2–3)
2026-10-04 | System fonts only (Segoe UI on Windows, Roboto on Android), no bundled web fonts | Both have full Greek coverage; no third-party services
2026-10-04 | Theme = CSS variables keyed on `<html data-theme>`, with separate light (cream, terracotta, saffron, basil) and dark (charcoal, ember, olive, wine) palettes and status-colour tokens | SPEC §3 visual direction; blobs animate `transform` only and stop under reduced motion
2026-10-04 | Greek status words for now: Σε αναμονή / Καθυστερεί / Ήρθε / Έφυγε / Έπρεπε να έχει φύγει | Short and plain; Minas can ask for other wording
2026-10-04 | `.claude/launch.json` not added | The real app is tested with `pnpm tauri dev`; a browser-only preview adds nothing yet
2026-10-04 | `.gitattributes` normalises text to LF (`.ps1` checked out as CRLF) | Same bytes on Windows and Linux, so `cargo fmt --check` and diffs agree
2026-10-04 | CI uploads the Windows debug NSIS installer as an artifact (kept 7 days) | Lets Minas try a build without compiling, if he wants
2026-10-04 | App icons are Tauri's default placeholders until the Phase 6 polish pass | Not part of any earlier phase

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

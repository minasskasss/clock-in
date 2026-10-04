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
2026-10-04 | Cloud sessions push to their assigned `claude/...` branch instead of `phase-N-name`; the stop report names the branch to test | The cloud GitHub proxy only allows pushes to the session's assigned branch

## Phase 1 — Core logic (cloud session, 2026-10-04)

2026-10-04 | Core deps: `uuid` 1 (serde), `sha2` 0.10 (already in the tree via Tauri), `unicode-normalization` 0.1; `proptest` 1 for tests | Small, pure crates; no I/O
2026-10-04 | Core outputs instants as `jiff::Timestamp` (not `Zoned`); civil times stay `civil::Time`/`Date`; weekday serialised as 1–7 (Mon = 1) | Cheap to compare and serialise; the timezone is always Europe/Athens anyway
2026-10-04 | DST: a nonexistent wall-clock time moves to the next valid minute (03:30 on 2027-03-28 → 04:00); an ambiguous one takes the first occurrence | SPEC §7.5; applied to block starts, ends and the rollover alike
2026-10-04 | A business day starts at the *instant* of its rollover (resolved with the same DST rules), and `business_date_for` compares instants | Keeps the business date monotonic even if the rollover falls inside the repeated fall-back hour
2026-10-04 | Rollover accepts any whole minute 00:00–08:00 in the core; the Settings UI may offer whole hours only | Spec calls it an "hour" but the server stores a `time`; the logic needn't care
2026-10-04 | Block length (15 min–16 h) and overlaps are checked in wall-clock minutes | Validation must not depend on the date; real length differs by ±1 h only on the two DST nights
2026-10-04 | "(+1)": shown on a start before the rollover, and on an end strictly after the midnight that closes the business date; an end of exactly 00:00 shows no "(+1)" | "19:00–00:00" reads as "until midnight" (SPEC §4.2 example); 00:00 is midnight, not "after midnight"
2026-10-04 | Overlap is checked only within one business day, not across neighbouring days | SPEC §4.2 wording; a cross-day clash would need a 16-hour block starting just before rollover
2026-10-04 | Alarm offsets are applied in real minutes to the block instants | "N minutes before/after" stays true across a DST change
2026-10-04 | Today-view statuses use block start/end, not the alarm offsets; a never-marked person stays amber after the end | SPEC §3 "past check-in time" / "past end time"
2026-10-04 | Today-view header shows the business date (not the calendar date) plus the Athens clock | At 01:30 the list shows the previous business day; the header matches it
2026-10-04 | Ordering ties broken by last name, then first name, accent- and case-insensitive (Greek tonos/dialytika and final sigma folded); ids break any remaining tie | SPEC §6; Latin names sort before Greek ones by code point
2026-10-04 | Alarm-event names in plan order: time, check-in before check-out, then "First Last" (accent-folded) | Deterministic; matches how the alarm screen lists names
2026-10-04 | Suppression is literal: only a check-in mark stops a check-in alarm, only a "left" mark stops a check-out alarm | SPEC §7.2; fail loud in odd correction cases
2026-10-04 | A removed person keeps a block that started before the removal (row and its alarms); later blocks disappear | SPEC §4.1 "from the next block onward"
2026-10-04 | Phase 2 note: `get_snapshot` must also return staff removed during the snapshot window (not only those with marks), and `schedule_set` should keep the ids of unchanged blocks | Otherwise an in-progress block of a just-removed person vanishes, and editing a schedule mid-shift detaches existing marks (row turns amber again)
2026-10-04 | Plan item id = first 32 hex chars of SHA-256 of `kind|staff_uuid|block_uuid|YYYY-MM-DD` (lowercase, hyphenated uuids); one value is pinned in a test | ARCHITECTURE §4; changing the format would orphan uploaded plans
2026-10-04 | Plan window is [now − 15 min, now + 14 days); `horizon_end` = the window end | A device firing a missed alarm up to 15 min late must still find the item in the plan (`check_alarm` treats a missing item as not due)
2026-10-04 | Missed-alarm window is inclusive: an alarm fires if it is 0–15 min late (15 min 0 s fires, 15 min 1 s doesn't) | SPEC §7.5
2026-10-04 | Ring cycle: a clock that went backwards counts as the first ring | Never silence an alarm because of a clock change
2026-10-04 | Override validation: date ≥ today's *business* date; day off has no blocks; replace hours needs ≥ 1 block | SPEC §4.3; an empty replacement is a day off
2026-10-04 | Names: Unicode NFC, trimmed, 1–40 chars, Greek (incl. polytonic) and Latin (incl. accented) letters, space, hyphen, `'` and `’`; at least one letter | SPEC §4.1; NFC so typed accents count as one character
2026-10-04 | Passphrase normalising and EFF-list checking live in the core (NFC → lowercase → trim/collapse whitespace; ≥ 5 words, ≥ 20 chars); errors report word positions, never words; "Generate" will pick indices with the OS RNG in Phase 3 | SPEC §4.6, ARCHITECTURE §5.3; testable in one place, nothing secret in errors or logs
2026-10-04 | Sync time checks (clock skew > 2 min, plan refresh when < 13 days or config changed, horizon banner < 3 days) are core functions | CLAUDE.md: platform layers never do time arithmetic
2026-10-04 | Sounds: additive synthesis, no samples. Check-in = bell-like C5–E5–G5 arpeggio at −1 dBFS; check-out = rounder G5–C5 at −4 dBFS with a slower attack; 2.0 s, 44.1 kHz 16-bit mono, cosine release to silence for a click-free loop | ARCHITECTURE §11; deterministic, so a test checks the committed WAVs match the generator (±1 LSB for cross-platform float differences)
2026-10-04 | `proptest-regressions/` is committed | proptest's recommendation: past failures are re-checked on every run
2026-10-04 | Rust toolchain pinned in `rust-toolchain.toml`: channel `1.99.0`, components clippy + rustfmt, profile minimal, no host triple or targets; both CI jobs run `rustup toolchain install` (reads the file) instead of `dtolnay/rust-toolchain@stable` | CI drifted to 1.99 while the cloud VM had 1.97, and a new clippy lint broke the build. One pinned version for the cloud VM, the Windows laptop (host stays `x86_64-pc-windows-msvc`) and CI. Bump on purpose and log it; Phase 5 adds the Android targets to the same file
2026-10-04 | **Phase 2 requirement, ARCHITECTURE §6 `check_alarm`:** it takes `(item_id, fires_at)` pairs and returns `due: true` only if `alarm_plan` holds that `item_id` with the **same** `fires_at` and no matching non-voided mark exists; otherwise `due: false`. It still returns `config_version`, so a device holding an older plan refreshes it | Item ids leave out the time and `schedule_set` keeps the ids of edited blocks, so after a 10:00 → 10:30 edit a stale Android alarm at 10:00 would otherwise find its id and ring (breaks PLAN Phase 5 checklist item 9). Offline is unchanged: no answer means ring (fail loud)
2026-10-04 | Kept ids time-free rather than adding `fires_at` to the item id | A stable id per (kind, person, block, date) can be the Android `PendingIntent` request code, so rescheduling a moved alarm replaces the old pending alarm instead of leaving it behind; the pair check gives the same protection as a time-based id
2026-10-04 | Same rule on the device side: `clockin-core` `is_still_due` / `AlarmEvent::still_due(plan, marks)` (replaces `without_marked`) keep an alarm only if the current plan has the same id and `fires_at` and no mark suppresses it. Windows re-rings (Phase 4) use it; Android (Phase 5) sends `fires_at` with each id to `check_alarm` | Without it, a block deleted, moved or turned into a day off during the silent gap would still re-ring (SPEC §7.2)
2026-10-04 | Phase 4/5 note: "already fired" / "stopped" state on a device is keyed by `(item_id, fires_at)` | A block moved later after its old time already rang must still ring at the new time
2026-10-04 | "Problems history" recorded in `docs/IDEAS.md` as a v1.1 proposal, not built in v1 | SPEC §10 puts history screens out of scope. Phase 2+ must not design around it, but must note in DECISIONS any choice that would make it much harder to add later

## Phase 2 — Backend and sync (local session, 2026-10-05)

2026-10-05 | Laptop tool versions: Windows 11 Home 10.0.26200, git 2.55.0, gh 2.102.0 (authenticated), rustup 1.29.1, rustc/cargo 1.99.0 MSVC (from `rust-toolchain.toml`), Node 24.19.0, pnpm 10.28.0 | First local session check (CLAUDE.md); nothing missing
2026-10-05 | Supabase CLI 2.119.0 is a pnpm dev dependency; migrations go to dev with `pnpm exec supabase db push --db-url "$env:CLOCKIN_DEV_DB_URL"` (dry run first). No `supabase/config.toml`, login or access token | Checked against the current CLI docs; history lives in `supabase_migrations.schema_migrations`
2026-10-05 | Publishable key sent only in the `apikey` header, never `Authorization: Bearer` | Supabase API-keys docs: the new keys are not JWTs; without a user session they map to `anon`
2026-10-05 | Grants are explicit: EXECUTE revoked from PUBLIC, `anon`, `authenticated` and `service_role` on every function, then granted to `anon` on exactly the 20 RPCs; schema `app` and its tables revoked from PUBLIC/`anon`/`authenticated`, default privileges too | Supabase stopped auto-granting Data API privileges to new objects (new projects from 2026-05-30, all projects from 2026-10-30), and Postgres grants EXECUTE to PUBLIC by default. A backend test checks the catalog
2026-10-05 | pg_cron enabled with `create extension pg_cron with schema pg_catalog` plus the grants from the Supabase Cron docs; job `clockin-purge` runs `app.purge()` daily at 01:17 UTC (cron times are GMT; `cron.schedule` upserts by name) | Supabase Cron docs; 03:17/04:17 in Greece, after closing
2026-10-05 | Sync client, local store and sync-loop logic live in a new crate `crates/clockin-sync`, not in `src-tauri` | Testable without Tauri, reused by the Android build; `src-tauri` keeps the timer, OS secret store and commands (Phase 3). ARCHITECTURE §3 layout updated
2026-10-05 | HTTPS via reqwest 0.13 with rustls + `ring` + `webpki-roots`, passed in with `tls_backend_preconfigured`; reqwest's default TLS (aws-lc-rs + platform verifier) is off | aws-lc-rs needs a C/NASM build that is harder to cross-compile for Android, and the platform verifier needs JNI setup on Android; Supabase's HTTPS endpoints use public CAs
2026-10-05 | The DB/pooler TLS certificates chain to Supabase's private "Supabase Root 2021 CA". The backend tests pin both copies of that root (`crates/clockin-sync/tests/supabase-root-ca.crt`, SHA-256 `807025ad…cafa` and `5f9b7795…9ae2`), which match the roots embedded in the official CLI and the chain the dev pooler sends | Full certificate verification for the connection that carries the DB password, instead of encrypt-without-verify
2026-10-05 | SQLCipher via rusqlite 0.40 `bundled-sqlcipher-vendored-openssl`; raw 256-bit key (`PRAGMA key = "x'…'"`); opening fails if `cipher_version` is empty or the key is wrong; WAL journal | Builds on the laptop (Strawberry Perl already installed) and on `windows-latest`; a test checks the file has no plain-text header or data
2026-10-05 | Local DB key generated with `getrandom` (OS CSPRNG); `DeviceSecret`, `SessionToken`, `QuitCode` and `StoreKey` have redacted `Debug` and no `Display` | CLAUDE.md: no credentials in logs, panics or errors
2026-10-05 | Server keeps at most one live mark per (staff, block, business date, kind) (partial unique index); `mark` is idempotent on the client id, and a second device marking the same occurrence gets the first mark back | Two devices tapping the same row must not create duplicate marks
2026-10-05 | `mark` accepts business dates from today − 2 to today + 1 (server business date); a queued mark the server refuses with `invalid_input` is dropped from the local queue | Offline marks may arrive late; one that can never be accepted must not block the queue
2026-10-05 | `schedule_set` and `override_set` keep block ids: a block sent with its `id` is updated in place; a block without an id reuses an identical existing block's id | Phase 1 note: editing a schedule mid-shift must not detach existing marks
2026-10-05 | `get_snapshot` returns overrides and marks from business date today − 2, and staff who are active, removed within 3 days, or referenced by a mark in that window; it also returns `this_device_id` | The core looks back two business dates (overnight blocks, missed alarms); a just-removed person's in-progress block must stay
2026-10-05 | Purge deletes marks and overrides with business date ≤ today − 30, removed staff once no marks reference them and they were removed over 2 days ago, expired sessions, and plan items that fired over a day ago | SPEC §9; the 2-day grace protects an in-progress block of someone just removed
2026-10-05 | Server cheap checks: names 1–40 chars after trimming; ≥ 1 block for `staff_create`/`schedule_set` (max 100), replace-hours 1–20 blocks, day off 0; whole-minute times, start ≠ end; offsets −60…30; rollover 00:00–08:00; quit code `^[0-9]{4}$`; passphrase ≥ 5 words, ≥ 20 chars and ≤ 72 bytes (bcrypt limit) | ARCHITECTURE §6: data integrity only; durations and overlaps stay in `clockin-core`
2026-10-05 | Lockout details: while locked, attempts are refused without checking the passphrase and don't count; the failure that starts a lock returns `bad_passphrase` with `retry_after_s`; a wrong *current* passphrase in `change_passphrase` counts too | SPEC §4.6; no way around the lockout
2026-10-05 | Version triggers are row-level (no bump when nothing changed); `devices` bumps `data_version` only for name, platform or revocation, not `last_seen_at` | `last_seen_at` changes every minute and would cause a snapshot refetch per minute
2026-10-05 | `admin_logout` needs only the device secret; `device_revoke` may revoke the calling device itself | Logging out must work with an expired session; self-revoke is harmless (the device just needs pairing again)
2026-10-05 | Sync backoff lives in core (`sync_retry_delay`: 5, 10, 30, then 60 s); the engine reports `online = false` only when the server is unreachable (network, HTTP or decoding error), not when it refuses a call | ARCHITECTURE §8; time logic stays in the core; the offline banner must mean offline
2026-10-05 | The build embeds the public config from `.env.dev` (debug builds) or `.env.prod` (release builds); `CLOCKIN_ENV=dev|prod` overrides; a missing file embeds empty values and the app treats itself as not configured | CI has no `.env` files; the app bundle holds only the URL and publishable key
2026-10-05 | Backend integration tests (`crates/clockin-sync/tests/backend.rs`) are `#[ignore]`d and run with `cargo test -p clockin-sync --test backend -- --ignored --test-threads=1`. They wipe dev's data (including any devices Minas paired to dev), refuse to run if the DB URL equals `CLOCKIN_PROD_DB_URL` or belongs to another project than `.env.dev`, and never print the URL. CI doesn't run them | Dev credentials stay off CI; prod is never a test target
2026-10-05 | Phase 6 note: Supabase Auth e-mail sign-ups are on by default. A signed-up user would get the `authenticated` role, which has no privileges on any RPC or on schema `app` (tested), but sign-ups should still be turned off in both projects during hardening | Defence in depth; nothing in Clock In uses Supabase Auth

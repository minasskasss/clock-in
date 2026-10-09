# Clock In — Architecture

## 1. Overview

```
 ┌──────────── Shop PC (Windows) ────────────┐      ┌──────── Employer phone (Android) ────────┐
 │ React UI (webview)                        │      │ React UI (webview, only while open)      │
 │   ↕ Tauri commands/events                 │      │   ↕                                      │
 │ Rust app core: sync loop, SQLCipher cache,│      │ Rust app core (same code, while open)    │
 │   scheduler, audio, tray, alarm window    │      │   ↕ Tauri plugin bridge                  │
 │   uses crates/clockin-core (pure logic)   │      │ Kotlin plugin: AlarmManager, ring        │
 └───────────────┬───────────────────────────┘      │   service, receivers, WorkManager        │
                 │ HTTPS (PostgREST RPC)            └───────────────┬──────────────────────────┘
                 ▼                                                  ▼
        ┌─────────────────── Supabase (EU, Frankfurt) ───────────────────┐
        │ Postgres only. Tables in schema `app` (not exposed), RLS on.   │
        │ SECURITY DEFINER RPC functions in `public` (exposed).          │
        │ No Supabase Auth, no Realtime, no Edge Functions.              │
        └────────────────────────────────────────────────────────────────┘
```

**Stack:**

- Tauri 2, Rust (stable, MSVC toolchain), React + TypeScript + Vite, pnpm.
- Rust crates:
  - `jiff` for timezone-correct time;
  - `reqwest` with rustls;
  - `rusqlite` with SQLCipher (`bundled-sqlcipher-vendored-openssl`);
  - `rodio` for audio;
  - `keyring-core` with `windows-native-keyring-store` (the keyring 4 family) for Windows Credential Manager;
  - `windows` for the Win32 calls.
- Tauri plugins: single-instance, notification, plus the custom Android plugin. The tray is Tauri's built-in `tray-icon`; start-with-Windows is a `Run` registry value the app writes itself (see §9).
- Kotlin for the Android alarm plugin.
- Supabase CLI as a dev dependency (`pnpm exec supabase ...`), used for migrations only.

**Development environment:**

- Phases 0–1 run in Claude Code **cloud sessions** (Ubuntu VM, fresh clone of the private repo `clock-in`).
- Phases 2–6 run in **local sessions** in the Claude desktop app on Windows, in `C:\dev\clock-in`.
- See `CLAUDE.md` (Environment) and `docs/SETUP.md` §3.

## 2. Key decisions and trade-offs

| Decision | Why | Cost / revisit when |
|---|---|---|
| Tauri 2 for both platforms | One codebase, the owner's preferred stack (Rust) | The Android alarm layer must be custom Kotlin |
| Supabase Postgres via RPC only, with a per-device secret instead of Supabase Auth | One auth mechanism that works the same from Rust and from Kotlin background code. Tables are not exposed at all. No token-refresh races between Rust and Kotlin. | Building our own lockout and sessions (simple, see §5) |
| Polling (5 s) instead of Realtime | Two devices, a few KB of data. Simpler and more robust. The always-running shop PC also keeps the free-tier prod project from pausing for inactivity. | Revisit if devices or data grow a lot |
| Server stores a **materialised alarm plan** computed by Rust | Scheduling logic lives once, in tested Rust. Kotlin background code downloads the plan and never recomputes it. | Some device must refresh the plan horizon (the always-on shop PC does this daily) |
| Pre-alarm server check on Android | Catches marks and schedule changes made elsewhere after the last background sync, without Firebase push | Rings if offline (fail loud, intentional) |
| Migrations via `--db-url` connection strings (session pooler) | No CLI login or access token is needed. The Session pooler works over IPv4. | The connection strings are secrets; see CLAUDE.md rules |
| Quit code is a plain 4-digit setting, checked locally | It only prevents accidental closing (owner decision) and works offline | Not a security control, by design |
| No auto-updater in v1 | Two devices, remote manual installs (AnyDesk / APK file) are fine | Add the Tauri updater if releases become frequent |

## 3. Repository layout

```
C:\dev\clock-in\
  CLAUDE.md
  docs/                        SPEC, ARCHITECTURE, PLAN, DECISIONS, SETUP, IDEAS, RECOVERY
  crates/clockin-core/         pure logic: schedules, business day, plan, validation, automatic theme
  crates/clockin-sync/         RPC client, SQLCipher store, sync-loop logic (no Tauri)
  src/                         React + TS UI; Greek only, every string in src/i18n/el.json
  src-tauri/                   Tauri app (Rust): sync loop, secrets, admin session, commands; scheduler, audio, tray (Phase 4)
  plugins/clockin-alarm/       Tauri plugin: Rust side + android/ (Kotlin)
  src-tauri/gen/android/       the generated Android Studio project (signing, minSdk, icons)
  supabase/migrations/         SQL migrations (schema, functions, grants, cron)
  assets/sounds/               generated check-in / check-out WAVs
  assets/icon/                 app icon sources (SVG): full design, hand-tuned 16/20/24/32/48 px, Android adaptive parts, icons.json
  assets/eff_large_wordlist.txt   (downloaded in Phase 0, SHA-256 recorded in DECISIONS)
  tools/gen-sounds/            small Rust bin that synthesises the sounds
  tools/generate-passphrase.ps1
  tools/android-env.ps1        Android build environment on Windows (JDK, SDK, NDK, MSYS2 Perl/make for OpenSSL)
  tools/build-apk.ps1          signed release APK, `-Env dev` (test phone) or `-Env prod` (employer)
  tools/android-install-debug.ps1  development loop: arm64 debug build installed on the USB phone
  tools/build-icons.mjs        `pnpm icons`: every icon in src-tauri/icons from assets/icon (tauri icon + an .ico with the tuned layers)
  .env.example                 SUPABASE_URL=, SUPABASE_PUBLISHABLE_KEY=   (committed, empty values)
  .env.dev / .env.prod         real values (gitignored)
```

## 4. Data model (Postgres, schema `app`)

| Table | Key columns |
|---|---|
| `staff` | `id uuid pk`, `first_name`, `last_name`, `removed_at timestamptz null`, timestamps |
| `schedule_blocks` | `id uuid pk`, `staff_id fk`, `weekday smallint 1–7` (Mon=1, business day), `start_time time`, `end_time time` |
| `overrides` | `id uuid pk`, `staff_id fk`, `business_date date`, `kind text ('off','replace')`, unique (`staff_id`, `business_date`) |
| `override_blocks` | `id uuid pk`, `override_id fk on delete cascade`, `start_time`, `end_time` |
| `settings` | singleton row: `checkin_offset_min`, `checkout_offset_min`, `rollover time`, `autostart bool`, `quit_code char(4)` |
| `marks` | `id uuid pk` (client-generated, makes inserts idempotent), `staff_id`, `source_block_id uuid`, `business_date`, `kind ('in','out')`, `marked_at timestamptz` (server time), `device_id`, `voided_at null` |
| `devices` | `id uuid pk`, `name`, `platform`, `secret_hash bytea` (SHA-256), `created_at`, `last_seen_at`, `revoked_at null` |
| `auth_state` | singleton: `passphrase_hash text null` (bcrypt), `failed_count int`, `locked_until timestamptz null` |
| `admin_sessions` | `token_hash bytea pk`, `device_id`, `expires_at` |
| `alarm_plan` | `item_id text pk`, `fires_at timestamptz`, `kind`, `staff_id`, `display_name`, `business_date`, `source_block_id` |
| `meta` | singleton: `config_version bigint`, `data_version bigint`, `plan_config_version bigint`, `plan_horizon_end timestamptz` |

**Versions:**

- `config_version` is bumped by triggers on staff, schedule blocks, overrides, override blocks and settings.
- `data_version` is bumped on every change, including marks and devices.

**Block identity:**

- A block occurrence is identified by (`source_block_id`, `business_date`).
- `source_block_id` is a `schedule_blocks.id` or an `override_blocks.id`.
- A plan `item_id` = hex(SHA-256(`kind | staff_id | source_block_id | business_date`)), truncated to 32 characters. It is deterministic, so recomputations keep the same ids.

## 5. Security

1. **Tables are unreachable.**
   - Tables live in schema `app`, which is **not** in the Data API's exposed schemas.
   - RLS is enabled on every table, with **zero policies** (defence in depth).
   - `anon` and `authenticated` have no privileges on schema `app`.
2. **Functions.**
   - The RPC functions live in `public`. Each is `SECURITY DEFINER` with `SET search_path = ''` and fully qualified names.
   - `EXECUTE` is revoked from `PUBLIC` on every function, and granted to `anon` only on the RPCs listed in §6.
   - Helper functions are never granted.
3. **Passphrase.**
   - Normalised by the client and again by the server: lowercase, trimmed, whitespace collapsed.
   - Stored as `extensions.crypt(p, extensions.gen_salt('bf', 12))`, using pgcrypto in the `extensions` schema.
   - bcrypt's 72-byte limit is fine: 5 EFF words come to at most about 50 bytes.
   - The server enforces at least 5 words and at least 20 characters. The client also checks every word against the bundled EFF list.
   - A 5-word EFF passphrase has about 64.6 bits of entropy.
4. **Lockout.**
   - Shared by `admin_login` and `pair_device`, stored in `auth_state`.
   - When `failed_count ≥ 5`, `locked_until = now() + 2^(failed_count−5) minutes`, capped at 60.
   - A success resets it.
   - ⚠️ **Gotcha:** a failed check must **return an error code, not `RAISE`**. Raising rolls back the transaction, which would undo the counter increment.
5. **Device secret.**
   - 32 random bytes (`gen_random_bytes`), base64url-encoded, returned once by `pair_device` or `admin_initialize`.
   - The server stores only its SHA-256.
   - Every device RPC checks it and that the device is not revoked, then updates `last_seen_at` (at most once a minute).
6. **Admin session.**
   - A 32-byte random token, stored as a hash.
   - Expires 10 minutes after the last admin call (sliding). Also tied to the device.
   - Invalidated by `admin_logout`, a passphrase change, or revoking the device.
7. **On-device secrets.**
   - **Windows:** device secret and SQLCipher key in Windows Credential Manager (DPAPI, user-scoped) via `keyring`.
   - **Android:** both are encrypted with an AES key held in Android Keystore and stored in app-private storage. The Kotlin plugin exposes `secretGet/secretPut` to Rust, and its background code reads them directly.
8. **Local database.** SQLCipher with a random 256-bit key. It holds the snapshot cache, the pending-marks queue and per-device settings. Its purpose is that copying the files is useless; the real protection of admin actions is the server-side check.
9. **The publishable key is public by design.** All it allows is calling the RPCs, and every RPC needs a device secret except `admin_initialize` (works only once) and `pair_device` (lockout-protected).
10. **The quit code is not a security control.** It is delivered to every paired device in the snapshot and compared locally.
11. **Accepted risk:** someone who extracts the publishable key from the app could deliberately trigger the lockout (a nuisance-level denial of service). A 64-bit passphrase cannot realistically be brute-forced through a 60-minute-capped lockout.

## 6. RPC contract (PostgREST `POST /rest/v1/rpc/<name>`)

Requests send the publishable key in the `apikey` header; verify the current key format and header in the Supabase docs.

All functions return `jsonb`: `{ ok: true, ... }` or `{ ok: false, error: "<code>", ... }`.
Error codes: `bad_secret`, `revoked`, `locked` (plus `retry_after_s`), `bad_passphrase`, `no_session`, `invalid_input` (plus `details`), `version_conflict`, `already_initialized`, `not_initialized`.

**Public**

- `admin_initialize(passphrase, quit_code, device_name, platform)` works only while `passphrase_hash` is null. It sets the passphrase and quit code, pairs the calling device, and returns `device_id` and `device_secret`.
- `pair_device(passphrase, device_name, platform)` returns `device_id` and `device_secret`.

**Device** (first argument `secret`)

- `get_version()` returns `config_version`, `data_version`, `plan_config_version`, `plan_horizon_end`, and `server_now`.
- `get_snapshot()` returns:
  - active staff, plus removed staff that still have marks in the window;
  - blocks;
  - overrides and override blocks from today onward;
  - settings, including `quit_code`;
  - non-voided marks for the last 2 business days and today;
  - devices (name, platform and `last_seen` only);
  - all versions, and `server_now`.
- `mark(client_id, staff_id, source_block_id, business_date, kind)` is idempotent on `client_id`.
- `upload_plan(base_config_version, horizon_end, items[])` replaces `alarm_plan` atomically. It returns `version_conflict` if `config_version` has changed since.
- `get_plan()` returns `plan_config_version`, `horizon_end` and the items from now − 15 min onward.
- `check_alarm(item_ids[])` returns, per item, `due: bool`. `due` is false if the item is gone from the plan or the matching mark exists. It also returns `config_version`.
- `admin_login(passphrase)` returns a `session_token`.

**Admin** (`secret`, `session_token`, …)

- `staff_create(first, last, blocks[])`, `staff_update(id, first, last)`, `staff_remove(id)`
- `schedule_set(staff_id, blocks[])` replaces the whole weekly template atomically.
- `override_set(staff_id, business_date, kind, blocks[])`, `override_delete(id)`
- `settings_update(...)` covers offsets, rollover, autostart and quit code.
- `mark_void(mark_id)` corrects a mistaken mark.
- `device_revoke(device_id)`
- `change_passphrase(old, new)`, `admin_logout()`

**Server-side validation:** the server checks types, ranges and lengths, that business dates are today or later, and that the quit code matches `^[0-9]{4}$`. Overlap and duration validation is done in `clockin-core` before the call. The server repeats only cheap checks; its validation exists for data integrity, not security.

**Maintenance:** a daily `pg_cron` job purges per SPEC §9 and expired admin sessions. Verify how pg_cron is enabled on the current Supabase platform.

## 7. clockin-core (pure Rust)

Inputs are plain structs (no I/O) and `now: jiff::Timestamp`. The timezone is fixed to `Europe/Athens`.

**Functions:**

- `business_date_for(now, rollover)`
- `occurrences(staff, blocks, overrides, business_date, rollover)` returns concrete start and end `Zoned` values. It handles split shifts, ends after midnight, starts before the rollover (+1 day), and DST: a nonexistent time moves to the next valid minute; an ambiguous time takes the earlier instant.
- `today_view(snapshot, now)` returns the ordered rows with their statuses.
- `alarm_plan(snapshot, from, horizon)` returns the items with deterministic ids, sorted.
- `due_events(plan, marks, window)` groups items by minute and applies suppression.
- `ring_cycle_state(started_at, now)` returns ringing, silent, or the next re-ring time.
- `validate_week(blocks)` and `validate_override(blocks)` check durations and overlaps.

## 8. Sync (Rust app core, while the app process is running)

- **Polling:** every 5 s, call `get_version`. If `data_version` changed, call `get_snapshot`, store it, and recompute.
- **Errors:** back off 5 → 10 → 30 → 60 s.
- **Pending marks:** queued in SQLCipher with their `client_id` and flushed on every successful contact.
- **Plan upkeep:** if `config_version > plan_config_version`, or `plan_horizon_end < now + 13 days`, compute the plan for [now, now + 14 days] and call `upload_plan`. On `version_conflict`, refetch and retry once.
- **Clock check:** compare `server_now` with the local clock and show the banner if they differ by more than 2 minutes. Alarm timing uses the device clock.
- **After any admin call that succeeds,** refresh immediately rather than waiting for the next 5-second tick.

## 9. Alarms — Windows

- **Scheduler loop** (a Rust task; every decision comes from `clockin-core::AlarmScheduler`):
  - find the next due event from the local plan and marks;
  - sleep until the earlier of that time and 30 s, then re-evaluate (robust to sleep/resume and clock changes);
  - fire immediately if the event is late by 15 minutes or less.
- **Firing:**
  - apply suppression from local state (at most 5 s old; fail loud if offline);
  - open the `alarm` window: always on top, focused, no close button, closable only via **Stop**;
  - loop the sound with `rodio`;
  - drive the repeat cycle from `ring_cycle_state`;
  - before each re-ring, recompute the names, and end the cycle if none are left.
- **Tray and startup:**
  - tray icon plus menu;
  - closing the main window hides it;
  - **Quit** shows a 4-digit keypad and compares against the cached `quit_code`. It works offline.
  - autostart: the app writes `"<exe>" --autostart` to the current user's `Run` key (quoted path; `tauri-plugin-autostart` writes it unquoted) and starts hidden in the tray; single instance via `tauri-plugin-single-instance`.
- **Sound and window together:** each scheduler decision sets both the sound and the alarm window (`alarms::Output`); the sound never plays without the window, and Stop or nobody left ends both.
- **Dead-key guard:** every app window is subclassed to drop `WM_DEADCHAR` / `WM_SYSDEADCHAR` before tao, which panics on one it did not see the key-down for (DECISIONS 2026-10-07).
- **Keep-awake:** `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` while running.
- **Mute detection:** Core Audio `IAudioEndpointVolume` (`GetMute`, master volume = 0), polled every 30 s, drives the banner.
- **Debug builds only:**
  - a `--profile <name>` flag allows a second instance with a separate data directory and credential namespace, for sync testing on one PC;
  - a fake-clock offset in a debug menu.

## 10. Alarms — Android (Kotlin plugin `clockin-alarm`)

**Commands exposed to Rust** (Rust calls them off the main thread; see DECISIONS, Phase 5):

- `setPlan(items, alertMode, configVersion, horizonEnd, theme, darkWindows)`: Rust sends its own 14-day plan without the alarms that marks known on the phone suppress, plus the alarm screen's theme and the automatic theme's dark periods (`clockin-core::auto_dark_windows`)
- `permissionStatus()`
- `alarmStatus()` (the cycle in progress, for the «Σταμάτημα» bar on Today) and `stopAlarm()`
- `diagnostics()` (the «Διαγνωστικά» view: phone, background refresh, next and last alarm)
- `openSettings(kind)` (notifications, exact alarms, full-screen, battery exemption, "pause if unused", the maker step, and Xiaomi's autostart, other-permissions and battery-saver screens)
- `setOemDone(done)`
- `secretGet/Set/Delete(name, value)`
- `setServerConfig(url, publishableKey)`
- `deviceName()`

**Persisted** (device-protected app storage, so alarms work after a reboot before the first unlock): the plan (times, kinds, staff names), the alert mode, the public server config, the handled alarms and the ring cycle in progress. The Keystore-encrypted secrets (device secret, local database key) stay in credential-encrypted storage, like the database itself: before the first unlock the pre-alarm check has no secret, so the alarm simply rings.

**Scheduling**

- Use `AlarmManager.setAlarmClock()`. Its show-intent opens the app. It survives Doze and the system exits idle just before it fires.
- Schedule every plan event in the next 48 h.
- Re-run scheduling on:
  - `setPlan`;
  - the worker refresh;
  - `BOOT_COMPLETED`;
  - `TIME_SET`;
  - `TIMEZONE_CHANGED`;
  - `MY_PACKAGE_REPLACED`.
- **Boot and time receivers only reschedule.** They never start a foreground service (Android 15+ forbids several FGS types from `BOOT_COMPLETED`).

**Exact-alarm permission**

- Declare `USE_EXACT_ALARM`; the APK is sideloaded and alarm-centric.
- Fall back to `SCHEDULE_EXACT_ALARM` plus `canScheduleExactAlarms()` checks.
- Verified (DECISIONS, Phase 5): `USE_EXACT_ALARM` is granted at install on Android 13+; `SCHEDULE_EXACT_ALARM` has `maxSdkVersion 32`.

**On fire** (`BroadcastReceiver`)

1. Start the ringing foreground service. Starting it from an exact alarm is an allowed background start. Its type is `systemExempted` on Android 14+ (allowed with an exact-alarm permission) and `mediaPlayback` on 10–13 (DECISIONS, Phase 5).
2. Call `check_alarm(item_ids)` with a 3 s timeout.
3. If nothing is due, stop silently. If `config_version` is newer than the stored plan's, enqueue a one-time plan refresh first.
4. **Ring mode:**
   - a high-importance notification with a full-screen intent opens `AlarmActivity` (`showWhenLocked`, `turnScreenOn`), with names and a large **Stop**, in the app's theme. The notification must not be "silent" (`setSilent`): SystemUI refuses full-screen intents for silent notifications and for suppressed group alerts (DECISIONS 2026-10-07). If it is swiped away (Android 14+), its delete intent posts it again while the cycle lasts. If a cycle started before notifications were allowed, Android shows nothing, so the permission check and the 25 s re-check post it once they are allowed. Today also shows a **Σταμάτημα** bar;
   - `MediaPlayer` loops with `AudioAttributes.USAGE_ALARM`;
   - while it rings, the server check repeats every 25 s: names marked, moved or removed elsewhere drop off the screen and notification, and the ring ends when none are left; a failed check keeps every name (fail loud);
   - after 5 minutes: stop the sound, remove the service, and schedule an exact re-ring 5 minutes later; the re-ring repeats the server check and drops marked names;
   - **Stop** ends the cycle for this event on this device;
   - if `canUseFullScreenIntent()` is false, still post the high-priority notification with the looping sound, and show the permission banner.
5. **Notification mode:** a single high-priority notification on the notification channel. No service, no repeat.

**Background refresh:** a WorkManager periodic job every 15 minutes with a network constraint. It calls `get_version`; if `plan_config_version` or `plan_horizon_end` changed, it calls `get_plan` and reschedules. If the horizon is under 3 days, it posts a quiet "open Clock In" notification.

**Notification channels:**

- `alarm` (max importance, alarm sound attributes);
- `reminders` (high);
- `service` (low).

**Onboarding checklist** (plain-language Greek, usable by a non-technical person on a phone call):

- notifications: `POST_NOTIFICATIONS` on Android 13+; below that, the app's notifications and its alarm channel switched on (`areNotificationsEnabled`);
- exact alarms (Android 12+);
- full-screen intent (Android 14+);
- battery-optimisation exemption (`REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`);
- "Pause app activity if unused" off (Android 12+);
- Kotlin reports the items that have no setting on the phone's Android version (`notApplicable`, always allowed there); the checklist leaves them out and «Διαγνωστικά» says they don't exist there. On Android 11 those are exact alarms, full screen and the unused-app switch;
- an OEM note with deep links for Xiaomi, Samsung, Huawei and Oppo autostart/background settings where they exist (see dontkillmyapp.com); Xiaomi gets four steps (autostart, other permissions, battery saver, lock in Recents).
- After an update (`MY_PACKAGE_REPLACED`), a notification if a required permission is off: Android 14+ installers may switch full-screen intents off on every update.
- **WebView:** the web build targets Chrome 91 (`vite.config.ts`); below that Android System WebView version the plugin's `load` shows a native Greek dialog on every start with a Play Store button (the web screens may not run at all), and «Διαγνωστικά» shows the version.

**HTTP client:** a minimal HTTPS POST to the RPC endpoint from Kotlin, using the stored publishable key and device secret.

## 11. Sounds

`tools/gen-sounds` synthesises two WAVs (44.1 kHz, about 2 s, seamless loop):

- **check-in:** a rising three-note major arpeggio with soft bell timbre;
- **check-out:** a falling two-note figure.

The WAVs are committed to `assets/sounds`. Android copies them to `res/raw`.

## 12. Testing

- **clockin-core:**
  - table-driven unit tests: split shifts, end after midnight, start before rollover, the rollover boundary (04:59 / 05:00 / 05:01), overrides (off, replace, adding a non-scheduled person), negative and positive offsets, grouping, suppression, the repeat cycle, ordering, and validation;
  - DST: 2026-10-25 (fall back) and 2027-03-28 (spring forward), including a shift spanning the change;
  - `proptest` for sortedness, id determinism and no duplicates.
- **Backend:** Rust integration tests against the **dev** project (`CLOCKIN_DEV_DB_URL` for setup and inspection; the RPCs over HTTPS with the dev publishable key):
  - schema `app` is unreachable through the Data API;
  - an unpaired call is denied;
  - lockout timing, and that the counter persists after a failed attempt;
  - idempotent marks;
  - `mark_void` is admin-only;
  - `upload_plan` version conflict;
  - admin calls without or with an expired session are denied;
  - the purge function.
  - Tests reset dev state themselves and never touch prod.
- **Frontend:** Vitest and React Testing Library for the Today-view states, the confirm dialogs and the quit-code keypad.
- **CI (GitHub Actions on every PR):**
  - a Linux job: fmt, clippy, Rust tests, and frontend typecheck, lint and tests;
  - a Windows job (`windows-latest`): clippy, Rust tests and a debug Tauri build.
  - The Windows job decides whether `#[cfg(windows)]` code is correct, which matters because cloud sessions compile on Linux only.
- **Manual:** the device checklists in PLAN.md.

## 13. Builds, releases and distribution

- **Windows:**
  - NSIS installer with the WebView2 bootstrapper, unsigned (SmartScreen warns once: "More info → Run anyway"), versioned with semver;
  - delivered to the shop PC over AnyDesk file transfer;
  - a new version installs over the old one and keeps data and pairing.
- **Android:**
  - a release APK signed with a keystore created in Phase 5 and stored **outside** the repo (`C:\dev\clock-in-keys\`). Minas backs up the keystore and its passwords;
  - delivered as a file (messaging app or Google Drive link);
  - a new APK signed with the same key installs as an update and keeps data and pairing;
  - a different key forces an uninstall, which loses pairing.
- **Android developer verification:** Google's developer verification for sideloaded apps applies to Greece from 2027. Before global enforcement, Minas registers in the Android Developer Console (a limited-distribution account is meant for small, personal distribution) and registers the package name and signing key. Re-check the requirements before each 2027 release.
- **Supabase:**
  - `clockin-dev` for development and tests, `clockin-prod` for real use;
  - migrations applied with `pnpm exec supabase db push --db-url "$env:CLOCKIN_<ENV>_DB_URL"`; confirm current CLI syntax.

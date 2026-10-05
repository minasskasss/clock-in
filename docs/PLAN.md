# Clock In — Build Plan

## How this plan is used

- **One phase per Claude Code session.**
  - **Phases 0–1 run in a cloud session** (SETUP §3B). They are pure code and need nothing from the laptop.
  - **Phases 2–6 run in a local session** (SETUP §3C): Claude desktop app, Code tab, Local, project `C:\dev\clock-in`, Auto mode, no worktree. They need the Supabase databases, the Windows app running, or Android devices.
  - Minas writes **"Start Phase N"** in a new session.
- **Claude Code** works through the phase on branch `phase-N-name`, opens a PR, and stops with the report format from CLAUDE.md. In local sessions it also leaves the branch checked out.
- **Minas** follows "Needs you", then runs the phase's checklist. He reports problems in the same session; Claude Code fixes them and re-reports. When everything passes, he writes **"approved, merge"**.
- **⛔ Needs Minas** marks a mid-phase stop for something only he can do. Claude Code prepares everything else first, so that stop is as short as possible. The click-by-click steps for each one are in `docs/SETUP.md`; Claude Code repeats the relevant steps in its report.

---

## Phase 0 — Scaffold

**Session: cloud.** The repo `clock-in` already exists, with the planning docs on `main`; do not create it. Run `check-tools` and record the versions in DECISIONS.

- The first commit on the branch adds `.gitignore`, covering:
  - `.env*` except `.env.example`;
  - `*.jks`, `*.keystore`, `*.pem`;
  - `target/`, `node_modules/`, `dist/`.
- Set up the Cargo workspace (`crates/clockin-core`, `src-tauri`, `plugins/clockin-alarm` stub, `tools/gen-sounds`).
- Set up the Tauri 2 + React + TS + Vite app with pnpm.
- Configure i18n (el default, en). *(Phase 3: English was dropped; the app is Greek only.)*
- Add theme tokens: CSS variables for light and dark, plus the background treatment from SPEC §3.
- Add `.env.example`.
- Add GitHub Actions on PRs:
  - a **Linux** job: fmt, clippy, Rust tests, frontend typecheck, lint and tests;
  - a **Windows** job (`windows-latest`): clippy, Rust tests and `pnpm tauri build --debug`. This job decides whether Windows-only code is correct.
- Download the EFF large wordlist into `assets/` and record its SHA-256 in DECISIONS.
- Keep `tools/generate-passphrase.ps1` as provided.
- Add `.claude/launch.json` only if useful for preview; the real app is tested with `pnpm tauri dev`.

**Checklist (Minas):**

1. On your laptop, test the branch (SETUP §3B, "Testing a cloud branch") → `pnpm tauri dev` opens a window with the themed background.
2. The light/dark toggle works, and the language toggle switches the placeholder text.
3. The PR on GitHub shows green checks, including the Windows job.

---

## Phase 1 — Core logic

**Session: cloud.**

- Implement everything in ARCHITECTURE §7, with the full test suite from §12.
- No UI or I/O work in this phase.
- Generate the two sounds (`tools/gen-sounds`).

**Checklist (Minas):**

1. Read the PR's summary of test cases (plain-language list).
2. Fetch the branch on your laptop (SETUP §3B), then play `C:\dev\clock-in\assets\sounds\check-in.wav` and `check-out.wav`. Approve them or ask for changes.

---

## Phase 2 — Backend and sync

**Session: local from here on.** Do SETUP §3C first.

⛔ **Needs Minas (at the start): SETUP §4, Supabase dev project.** When he's done:

- the dev project URL and publishable key are pasted into the chat;
- `CLOCKIN_DEV_DB_URL` is set in the local environment editor;
- a new session is started if the variable isn't visible.

Then build:

- migrations: schema `app`, triggers, RLS, grants, every RPC from ARCHITECTURE §6, and `pg_cron` purge;
- apply them to dev;
- the Rust sync client and the SQLCipher store;
- backend integration tests against dev.

**Checklist (Minas):**

1. Claude Code shows the integration-test output: all passing.
2. In the Supabase dashboard (dev) → Table Editor → schema `app`: the tables exist.
3. In the dashboard → API docs / Data API: no `app` tables are listed as accessible.

---

## Phase 3 — Windows UI

- **First-run screens (plain language, Greek):**
  - **"Set up as first device":** the employer enters the passphrase twice (validated against the EFF list), then a 4-digit quit code twice. This calls `admin_initialize`.
  - **"Pair this device":** calls `pair_device`.
- **Today view:** full SPEC §6, including the confirm dialogs and banners. No undo.
- **Settings (passphrase-gated):**
  - staff create, edit and remove;
  - weekly schedule editor with split shifts and "(+1)" display;
  - overrides (day off / replace hours);
  - offsets and rollover;
  - Today's marks (remove a mistaken mark);
  - devices (revoke);
  - a **Κωδικοί** section: change passphrase with **Generate**, and change quit code;
  - lockout UI.
- **Menu:** theme (Αυτόματο, Σύστημα, Φωτεινό, Σκοτεινό). Greek only, no language choice.
- **Static background:** no background animation, cheap to draw (SPEC §3).
- **Debug only:** `--profile` second instance and the fake-clock offset.

**Checklist (Minas, on his laptop, dev project):**

1. Fresh start → "Set up as first device" → set a throwaway passphrase and the quit code `1234` → the app reaches the Today view.
2. Start a second instance with `--profile b` (Claude Code gives the exact command) → pair it with the passphrase.
3. Add Staff A: Mon–Fri 09:00–17:00.
4. Add Staff B: today 12:00–16:00 + 19:00–02:00. Check that B shows twice and that 02:00 shows "(+1)".
5. Add an override: A has a day off today → A disappears from today. Delete the override → A comes back.
6. Add an override: Staff C (who doesn't work today) gets 10:00–14:00 → C appears.
7. In instance 1, tap A → the confirm dialog appears → confirm → green. Within 10 s it is green in instance 2.
8. Tap A again → confirm "left" → the red line appears.
9. In Settings → Today's marks, remove A's "left" mark → the red line disappears on both instances.
10. Enter the wrong passphrase 5 times → locked with a countdown. Restart the app → still locked.
11. Change the passphrase → the old one fails and the new one works. Generate a passphrase → it is 5 words.
12. Change the quit code in Settings → **Κωδικοί** → it saves.
13. Switch the theme between Αυτόματο, Σύστημα, Φωτεινό and Σκοτεινό. With the fake clock, Αυτόματο turns dark at 21:00 and light at the rollover. There is no English anywhere.
14. Revoke instance 2 from instance 1 → instance 2 shows the pairing screen.
15. Try to make an override for yesterday and an overlapping block → both are refused with a clear message.

---

## Phase 4 — Windows alarms and installer

- Everything in ARCHITECTURE §9: scheduler, alarm window, sounds, repeat cycle, tray, quit-code keypad, autostart, single instance, keep-awake, mute banner, clock-skew banner, missed-alarm rule.
- NSIS installer.
- Tray icon: the hand-tuned small layer of `icon.ico` at 16 px × the display scale (`tauri::image::Image::from_app_icon_resource`), not the 512 px design shrunk (DECISIONS, Phase 3 icon).

⛔ **Needs Minas (end of phase): SETUP §5, Supabase prod project.** When he's done:

- the prod URL and publishable key are pasted into the chat;
- `CLOCKIN_PROD_DB_URL` is set.

Claude Code then applies the migrations to prod and builds the installer against prod.

**Checklist (debug build with fake clock first, then the installed prod build on his laptop):**

1. Close the window → the tray icon remains. At alarm time the alarm window appears on top and loops the **check-in** sound.
2. Don't press Stop → it goes silent after 5 minutes and rings again at +10. Press Stop → it ends.
3. Mark the person during the silent gap → no re-ring.
4. Mark a person 2 minutes before their alarm → no alarm.
5. Two people due at the same minute → one alarm showing both names.
6. A check-in and a check-out in the same minute → one alarm with both sections, playing the check-in sound.
7. Check-out alarm → the **check-out** sound.
8. Restart the laptop → after login, the app starts by itself into the tray.
9. Tray → Quit → asks for the 4-digit code. A wrong code keeps it running; the right code quits. Repeat with Wi-Fi off → still works.
10. Mute Windows → the banner appears.
11. Fake clock to 2026-10-25 around 03:00–04:00 with a shift spanning it → correct alarm times.
12. Turn Wi-Fi off → the offline banner appears and alarms still fire. Mark someone offline, reconnect → the mark syncs.

**After merge:** go live on the shop PC (SETUP §7). From then on, prod is in real use, and later phases must not break it.

---

## Phase 5 — Android

⛔ **Needs Minas (start): SETUP §6, Android Studio and a test phone.**

The test device is Minas's own Android phone (or any spare Android phone in Athens). If none is available, use the Android emulator for the basics, and run the OEM-specific checks on the father's phone during go-live.

Then build:

- Tauri Android target;
- the full Kotlin plugin from ARCHITECTURE §10;
- onboarding checklist;
- per-device alert-mode setting;
- app icon: copy `src-tauri/icons/android/` (made by `pnpm icons`: adaptive foreground and background, themed monochrome, legacy and round) into the Android project's `res/`;
- per-device theme, the same as Windows (SPEC §3, §8.2): Αυτόματο by default (the rule comes from `clockin-core`), Σύστημα following Android's dark theme, Φωτεινό, Σκοτεινό; the same static background.

The test phone is paired to **dev** during development.

⛔ **Needs Minas (end).** Claude Code generates the release keystore in `C:\dev\clock-in-keys\` (outside the repo). Minas copies the keystore file and its passwords to two safe places (password manager + USB stick or cloud drive).

**Checklist (test phone, installed release APK):**

1. Install the APK from a file (the same way his father will) → it installs.
2. Onboarding shows each permission with ✓ / ✗. Grant all of them → everything is ✓.
3. Ring mode: lock the phone, swipe the app away, wait for the alarm → the full-screen alarm appears over the lock screen and loops until Stop.
4. Don't press Stop → silent after 5 minutes, re-rings at +10.
5. Notification mode: a single notification with the names, no loop.
6. Reboot the phone and don't open the app → the next alarm still fires.
7. Airplane mode → the alarm still rings.
8. On the laptop app (same dev project), mark the person 2 minutes before their alarm → the phone stays silent.
9. On the laptop, move a shift from 10:00 to 10:30 while the phone is locked → nothing at 10:00, rings at 10:30.
10. Battery saver on → the alarm still fires.
11. Do Not Disturb on → note what happens and report it.
12. Leave the phone idle overnight → the morning alarm fires on time.
13. Install a newer APK over the old one → data and pairing are kept.

---

## Phase 6 — Hardening and handover

- Review the security checklist in ARCHITECTURE §5 item by item, and record the result in the PR.
- Test offline for longer than 1 hour, then reconnect.
- Run the purge job manually on dev and verify it.
- Visual polish pass on both platforms.
- Write `docs/RECOVERY.md` for Minas (non-technical, step by step):
  - reset a forgotten passphrase (Supabase SQL editor);
  - re-pair a device;
  - change the quit code if forgotten (via passphrase → Settings);
  - install a new version on the shop PC (AnyDesk) and on the phone (APK file);
  - unpause a Supabase project;
  - what to do if the keystore is lost;
  - the 2027 Android developer verification steps.
- Write a short `README.md`.
- Tag `v1.0.0`. Build the final prod installer and release APK.

**Checklist:** rerun SPEC §11 acceptance criteria 1–9 on the real devices (shop PC via AnyDesk, the father's phone after SETUP §8).

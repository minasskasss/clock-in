# Clock In — Product Spec (v1)

## 1. Purpose

Greek law requires employees to clock in and out with the digital work card (ψηφιακή κάρτα εργασίας). Clock In reminds the staff of a family pizzeria in Rhodes, and the employer, to do this at the right times.

Clock In is **only a reminder**. It is not the legal record, and it does not connect to ERGANI.

## 2. Devices and roles

- **Devices in normal use:** the **shop PC** (Windows 10, in Rhodes) and the **employer's Android phone**. Nothing should hard-limit the number of devices, but v1 has no per-device staff filtering: every paired device alerts for every staff member.
- **Remote setup:** both devices are set up remotely by Minas (Athens). The shop PC is set up over AnyDesk. The phone gets the APK as a file, with Minas guiding (see `docs/SETUP.md` §8). Setup screens must therefore be simple and self-explanatory.
- **Pairing:** each device must be **paired once** using the employer's passphrase. An unpaired install can do nothing except show the pairing screen. The employer can revoke a paired device from Settings.
- **Roles:**
  - **Anyone at a paired device** can see the Today view, mark check-ins and check-outs, and stop a ringing alarm.
  - **Employer (admin)** unlocks **Settings** with the passphrase. Settings re-lock on leaving the screen or after 5 minutes of inactivity. The staff never learn the passphrase.

## 3. Language and look

- **Language:** Greek by default, with an English toggle.
- **Theme:** follows the system light/dark setting by default, with a manual override.
- Language and theme live in a small menu that does **not** need the passphrase.
- **Visual direction:** calm, warm and polished.
  - Soft atmospheric background (blurred gradient shapes with subtle grain), with distinct palettes for light and dark.
  - Large, legible names, readable from about 2 m away on the shop PC.
  - Subtle transitions, with `prefers-reduced-motion` respected.
- **Status colours:**

| State | Look |
|---|---|
| Pending | neutral |
| Due or late (past check-in time, not marked) | amber |
| Checked in | green |
| Left | green row with a red line through it |
| Should have left (past end time, not marked out) | amber accent on a green row |

## 4. Data the employer manages (Settings, passphrase-protected)

### 4.1 Staff

- **Required fields:** first name, last name, and a weekly schedule with at least one time block.
- **Names:** 1–40 characters each, trimmed. Greek and Latin letters, spaces, hyphens and apostrophes are allowed.
- **Removing a staff member:**
  - needs a confirmation dialog;
  - the person disappears from today (from the next block onward) and from all future days;
  - existing marks are kept until the retention purge (§9).

### 4.2 Weekly schedule

- Each weekday (Monday–Sunday) has **zero or more time blocks** (start–end). Several blocks on one day is a **split shift**, e.g. 12:00–16:00 and 19:00–00:00.
- **Time format:** 24-hour `HH:MM`, always Greek local time (Europe/Athens), whatever the device's timezone setting says.
- **Weekdays are business days** (§5).
  - **End earlier than or equal to start** means the block ends the next calendar day, e.g. 18:00–02:00. "00:00" as an end time is allowed.
  - **Start earlier than the rollover hour** means the block starts after midnight, on the night that follows that business day. The editor shows "(+1)" next to any time that falls after midnight.
- **Validation:**
  - a block lasts 15 minutes to 16 hours;
  - start equal to end is invalid;
  - one person's blocks on the same business day must not overlap.

### 4.3 One-off changes (date overrides)

For a specific person and a specific business date (today or later), the employer can choose one of:

- **Day off:** no blocks that day.
- **Replace hours:** a custom set of blocks for that date. This is also how he adds someone who doesn't normally work that day.

Overrides leave the weekly template untouched. Settings lists upcoming overrides, each of which can be deleted. Past overrides are purged automatically (§9).

### 4.4 Settings values

| Setting | Default | Range |
|---|---|---|
| Check-in alarm offset (minutes relative to block start; negative means before) | 0 | −60 … +30 |
| Check-out alarm offset (minutes relative to block end) | 0 | −60 … +30 |
| Business-day rollover hour | 05:00 | 00:00 … 08:00 |
| Start with Windows (shop PC) | on | — |
| Quit code (Windows, see §8.1) | set at first run | exactly 4 digits |

The repeat cycle (ring 5 minutes, silent 5 minutes) is fixed in v1.

### 4.5 Today's marks (correction)

Settings lists today's marks. The employer can **remove** a mistaken mark, for example someone tapped the wrong name. There is no undo outside Settings.

### 4.6 Passphrase

- **First run:** the passphrase is set on the first device during setup. This works only while no passphrase exists yet.
- **Format:** at least 5 words, each from the EFF large wordlist, separated by spaces. Input is normalised before checking: trimmed, lowercased, repeated spaces collapsed to one, Unicode NFC.
- **Change passphrase:** needs the current passphrase plus the new one entered twice. Settings offers a **Generate** button that builds a 5-word passphrase using the OS's cryptographic random generator. A passphrase generated elsewhere (e.g. `tools/generate-passphrase.ps1` or dice) may be typed in instead.
- Changing the passphrase signs out every admin session. Paired devices stay paired.
- **Wrong attempts:**
  - after 5 consecutive wrong attempts on any device, attempts lock for 1 minute;
  - each further failure doubles the lock, up to 60 minutes;
  - a correct entry resets the count;
  - the UI shows the remaining lock time.
- **Recovery:** if the passphrase is forgotten, Minas resets it from the Supabase dashboard (documented in `docs/RECOVERY.md`, written in Phase 6). There is no in-app recovery.

## 5. Business day

The **business day** runs from the rollover hour on day D to the rollover hour on D+1 (default 05:00 → 05:00).

- A block belongs to the business day in which it **starts**.
- The Today view always shows the business day that contains "now". At 01:30 on Tuesday, for example, it still shows Monday's business day, including a shift running 18:00–02:00.

## 6. Today view (main screen)

- **Header:** weekday, date, and a live clock (HH:MM).
- **List:** every block of the current business day, ordered by start time, then last name. Someone with a split shift appears once per block.
- **Row contents:** full name, block time (with "(+1)" where it applies) and status (§3).
- **Marking:**
  - Tap a **pending** row → confirm "Mark ⟨name⟩ as checked in?" → the row turns green.
  - Tap a **checked-in** row → confirm "Mark ⟨name⟩ as left?" → a red line appears through the row.
  - Tapping a **left** row does nothing.
  - There is no undo. Only the employer can correct marks (§4.5).
- **Marks can be made at any time**, including before the scheduled time. A mark made before an alarm is due **prevents that alarm on every device** (§7.2).
- **Empty state:** "Κανείς δεν δουλεύει σήμερα" (EN: "Nobody is working today").
- **Banners, shown only when relevant:**
  - offline, with the last sync time;
  - device clock off from server time by more than 2 minutes;
  - (Windows) sound output muted or at zero;
  - (Android) a required permission is missing, with a button to fix it;
  - alarm schedule horizon running short (fewer than 3 days covered).
- A gear icon opens the passphrase prompt for Settings.

## 7. Alarms

### 7.1 When alarms fire

- For every block occurrence:
  - **check-in alarm** at block start + check-in offset;
  - **check-out alarm** at block end + check-out offset.
- **Grouping:** alarms falling in the same minute become one alarm event, listing the names under "Check in" and "Check out".
- **Sounds:** two distinct, original, generated sounds (no third-party audio):
  - **check-in:** bright and rising;
  - **check-out:** softer and falling.
  - An event that contains any check-in plays the check-in sound; otherwise it plays the check-out sound.

### 7.2 Suppression

- A check-in alarm does not fire if that person is already marked **checked in** for that block.
- A check-out alarm does not fire if that person is already marked **left** for that block.
- A check-out alarm **still fires** if the person was never marked in.
- If a person's alarm was removed by a schedule change (block deleted or moved, day off), it does not fire.
- **Fail loud:** if a device cannot confirm the current state (offline, server unreachable), it rings from its last known schedule.

### 7.3 Ring mode (Windows always; Android when chosen)

- The sound loops while a full-screen (Android) or always-on-top (Windows) alarm screen shows the names and one large **Stop** button.
- **Repeat cycle:**
  - rings for 5 minutes;
  - if not stopped, stays silent for 5 minutes and then rings again;
  - this repeats until **Stop** is pressed on that device.
- Before each repeat, names already marked are dropped. If no names are left, the cycle ends by itself.
- **Stop affects only the device it was pressed on.**
- **Stop does not mark anyone.** Marking is always done on the row (§6).

### 7.4 Notification mode (Android option)

- One notification per alarm event, listing the names under "Check in" and/or "Check out".
- It uses the standard notification sound, with no looping and no repeats.
- Tapping it opens the app.

### 7.5 Reliability requirements

- **Windows:** alarms fire while the app is hidden in the tray.
- **Android:** alarms fire when the app has been swiped away, the phone is locked, after a reboot, and in battery-saver or Doze.
- **Missed alarms:** if the device was asleep or off at alarm time and becomes available within 15 minutes, the alarm fires immediately. After 15 minutes it is skipped.
- **Daylight saving:**
  - spring-forward: a time that doesn't exist fires at the next valid minute;
  - fall-back: an ambiguous time fires at its first occurrence.

## 8. Platform behaviour

### 8.1 Windows (shop PC)

- Starts with Windows at user login (admin setting, on by default). Single instance only.
- Closing the window hides it to the tray. The tray menu has **Open** and **Quit**.
- **Quit asks for the 4-digit quit code.**
  - The code only prevents accidental closing. It is **not** a security measure, and staff may know it.
  - The employer sets it during the first-run setup on Windows and can change it in Settings.
  - It is synced with the other settings and checked locally, so it also works offline.
  - There is no lockout on wrong codes.
- While the app runs, Windows is kept from idle-sleeping (manual sleep and shutdown are still possible).

### 8.2 Android (employer's phone)

- **Per-device settings, no passphrase needed:** alert mode (**Ring** / **Notification**, default Ring), language and theme.
- **First-run onboarding** walks through each required permission and the battery-optimisation exemption. It shows a live checklist (✓ / ✗) that is also available later in the menu. The wording must be clear enough for a non-technical person following instructions over the phone.
- **Distribution:** a signed APK (not Google Play). New versions install over the old one and keep data and pairing.

## 9. Data retention and privacy

- **Stored data:** only first name, last name, schedules, overrides and marks. No phone numbers, IDs or other personal data.
- **Retention:**
  - marks and overrides are deleted 30 days after their business date;
  - removed staff are deleted once no marks reference them.
- The server is hosted in an EU region (Frankfurt).

## 10. Out of scope for v1

- ERGANI integration;
- hours or payroll reports;
- history screens;
- staff phones and per-person alert filtering;
- auto-updates;
- iOS and macOS;
- multiple shops.

## 11. Acceptance criteria (must all pass before v1 ships)

1. A schedule change made on either device appears on the other within 10 seconds while both apps are open. On a backgrounded Android phone it is applied before the next affected alarm when online, and within 15 minutes otherwise.
2. A check-in marked on the shop PC at least 1 minute before the alarm time prevents the alarm on both devices.
3. On Android, with the app swiped away, the screen locked and the phone just rebooted, a Ring-mode alarm fires within 60 seconds of its time and loops until Stop is pressed.
4. On Windows, with the window closed to the tray, the alarm fires and loops, then re-rings after 5 silent minutes until Stop.
5. Split shifts, a shift ending after midnight, a day-off override and a replace-hours override all show correctly in the Today view and produce the correct alarms. This includes the 2026-10-25 daylight-saving change.
6. An unpaired install cannot read or write any data. A paired device without the admin session cannot change staff, schedules, overrides, settings or devices, and cannot remove marks, even if the app is modified.
7. The passphrase lockout triggers after 5 wrong attempts and survives an app restart.
8. With the network off, alarms still fire. Marks made offline sync automatically once the device reconnects.
9. Quitting the Windows app requires the correct 4-digit quit code, including while offline.

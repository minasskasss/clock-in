# Clock In — Recovery and maintenance guide (for Minas)

Plain, step-by-step instructions for when something goes wrong or needs
updating. Written for the shop PC (Windows 10, Rhodes, over AnyDesk) and
your dad's phone (Redmi Note 11S, MIUI 13, Android 11).

**Never paste into any chat:** the passphrase, the quit code, the database
passwords, the connection strings, or the keystore passwords.

Contents

1. [The passphrase is forgotten](#1-the-passphrase-is-forgotten)
2. [Passphrase locked out (too many wrong tries)](#2-passphrase-locked-out-too-many-wrong-tries)
3. [Re-pair a device](#3-re-pair-a-device)
4. [The quit code is forgotten](#4-the-quit-code-is-forgotten)
5. [Install a new version on the shop PC (AnyDesk)](#5-install-a-new-version-on-the-shop-pc-anydesk)
6. [Install a new version on the phone (APK file)](#6-install-a-new-version-on-the-phone-apk-file)
7. [Clock In crashed on the shop PC: read crash.log](#7-clock-in-crashed-on-the-shop-pc-read-crashlog)
8. [Check the phone remotely: «Διαγνωστικά»](#8-check-the-phone-remotely-διαγνωστικά)
9. [The phone's alarms stopped: the MIUI settings](#9-the-phones-alarms-stopped-the-miui-settings)
10. [Update Android System WebView](#10-update-android-system-webview)
11. [A Supabase project is paused](#11-a-supabase-project-is-paused)
12. [The signing key is lost or broken](#12-the-signing-key-is-lost-or-broken)
13. [Android developer verification (2027)](#13-android-developer-verification-2027)
14. [Set up a new laptop for building Clock In](#14-set-up-a-new-laptop-for-building-clock-in)

---

## 1. The passphrase is forgotten

There is no "forgot passphrase" button in the app. You set a new one in the
Supabase dashboard. Paired devices stay paired; only Settings need the new
passphrase.

1. On your laptop, make a new passphrase. In PowerShell:

   ```powershell
   cd C:\dev\clock-in
   powershell -ExecutionPolicy Bypass -File .\tools\generate-passphrase.ps1 -WordList .\assets\eff_large_wordlist.txt
   ```

   Save it in your password manager.
2. Open supabase.com → sign in → open **clockin-prod** (the real one).
3. Left sidebar → **SQL Editor** → **+ New query**.
4. Paste this, then replace `NEW PASSPHRASE HERE` with the new passphrase
   (keep the single quotes around it):

   ```sql
   update app.auth_state
      set passphrase_hash = extensions.crypt(app.normalize_passphrase('NEW PASSPHRASE HERE'), extensions.gen_salt('bf', 12)),
          failed_count = 0,
          locked_until = null
    where id;
   delete from app.admin_sessions where true;
   ```

5. Click **Run**. It should say "Success. No rows returned".
6. **Delete the query you just ran**, because it contains the passphrase:
   in the SQL Editor's list on the left, find the query (usually "Untitled
   query"), click **⋯** next to it → **Delete query**.
7. Phone your dad and read him the new passphrase slowly. He writes it on
   paper and keeps it away from the shop.
8. Test: on the shop PC (AnyDesk), click the gear ⚙ → type the new
   passphrase → Settings open.

## 2. Passphrase locked out (too many wrong tries)

After 5 wrong tries the passphrase is locked for 1 minute, then 2, 4, … up to
60 minutes. The app shows the time left. Usually: just wait.

To unlock at once (without changing the passphrase):

1. supabase.com → **clockin-prod** → **SQL Editor** → **+ New query**.
2. Paste and **Run**:

   ```sql
   update app.auth_state set failed_count = 0, locked_until = null where id;
   ```

3. If the lockouts keep coming back and nobody at the shop is trying, tell
   Claude Code: someone may be guessing.

## 3. Re-pair a device

A device needs pairing again if it was disconnected («Αποσύνδεση»), if the
app was uninstalled, or on a new PC or phone. It then shows «Καλώς ήρθατε στο
Clock In».

1. On the device: **«Σύνδεση αυτής της συσκευής»**.
2. Type the passphrase (from the paper or your password manager) → **«Σύνδεση»**.
3. On a phone: the «Ρυθμίσεις τηλεφώνου» checklist opens; make every item ✓
   (section 9).

If the device was **lost or stolen**: on the other device, gear ⚙ →
passphrase → **Ρυθμίσεις → Συσκευές** → **«Αποσύνδεση»** next to it →
**«Ναι, αποσύνδεση»**. It can no longer read or change anything.

Never choose «Ρύθμιση ως πρώτη συσκευή» again on prod: it only works once,
and it already happened at go-live.

## 4. The quit code is forgotten

The quit code only stops the shop PC app from being closed by accident.

1. On the shop PC (AnyDesk): gear ⚙ → passphrase → **Ρυθμίσεις → Κωδικοί**.
2. Under «Κωδικός εξόδου», type a new 4-digit code twice →
   **«Αλλαγή κωδικού εξόδου»**. The old code is not needed.
3. It syncs to every device within seconds, also the phone (it isn't used there).

**Closing the app without the code** (for example if Settings can't open):
Clock In now runs with a small helper that restarts it if it closes, so
ending it in Task Manager alone brings it back after 2 seconds. To stop both:

1. Press **Ctrl+Shift+Esc** (Task Manager) → **More details** if needed →
   tab **Details**.
2. There are two `clock-in.exe` lines. Right-click either → **End process tree**.
   Do the same for the other if it is still there.
3. To start it again: Start menu → **Clock In**.

## 5. Install a new version on the shop PC (AnyDesk)

Updates keep the pairing, the settings and today's marks. Do it outside
opening hours if you can.

**One-time: AnyDesk unattended access** (so you can connect without anyone
at the PC):

1. On the shop PC (with someone there, or over a normal AnyDesk session):
   AnyDesk → **☰ menu → Settings → Security**.
2. **Unlock security settings** (Windows asks for permission → **Yes**).
3. Tick **Enable unattended access** → set a long, unique password → save it
   in your password manager.
4. Note the shop PC's AnyDesk address in your password manager too.
5. From now on: on your laptop type the address → **Connect** → enter that
   password.

**Each update:**

1. Claude Code puts the installer in `C:\dev\clock-in-releases\<version>-prod\`
   (for example `Clock In_1.0.0_x64-setup.exe`). Use only files from a
   `-prod` folder.
2. AnyDesk → connect to the shop PC.
3. AnyDesk's **File transfer** (toolbar icon with the folder, or right-click
   the tab → File Transfer) → on the left your laptop, on the right the shop
   PC → copy the installer into the shop PC's **Downloads** folder.
4. On the shop PC: open **Downloads** → double-click the installer.
   - If **"Windows protected your PC"** appears: **More info → Run anyway**.
5. If it says Clock In is running: click **OK** (it closes Clock In and its
   helper by itself).
6. **Next / Install** through the pages. On the last page leave
   **Run Clock In** ticked → **Finish**.
7. Check: the Today list shows the same people as before, and no
   «Καλώς ήρθατε» screen. Menu ☰ shows nothing different; the version is in
   the installer's file name.
8. Delete the installer from **Downloads**.

**Never uninstall first.** Uninstalling with "Delete the application data"
removes the local data; the pairing itself survives in Windows' Credential
Manager, but there is no reason to risk it.

## 6. Install a new version on the phone (APK file)

1. Claude Code puts the APK in `C:\dev\clock-in-releases\<version>-prod\`
   (for example `clock-in-1.0.0.apk`). Use only the file from a `-prod`
   folder; files with `-dev` in the name are for your test phone.
2. Send it to your dad as a **file** (Viber or WhatsApp: attach → File/Document,
   not as a photo), or as a Google Drive link.
3. Tell him (SETUP §8.2 has the words to use):
   1. Tap the file in the chat → **Install / Ενημέρωση**.
   2. If MIUI asks to allow installs from Viber/WhatsApp: allow it, go back,
      tap the file again.
   3. If MIUI or Google Play Protect warns: **More details / Περισσότερα →
      Install anyway / Εγκατάσταση ούτως ή άλλως**.
   4. Open Clock In. It is still paired.
4. Ask him for a screenshot of **☰ → «Διαγνωστικά»** (section 8): the version
   is the new one and every setting is ✓ (or «δεν υπάρχει σε αυτή την
   έκδοση Android»).

**Never uninstall first**: it loses the pairing and the phone settings.

## 7. Clock In crashed on the shop PC: read crash.log

If Clock In closes by itself, its helper starts it again within seconds and
writes one line to `crash.log`. After 5 crashes within 10 minutes it stops
trying and shows a message on the screen («Clock In: έκλεισε»).

To read it:

1. AnyDesk → shop PC → press **Win+R**.
2. Type `%LOCALAPPDATA%\io.github.minasskasss.clockin` → **OK**.
3. Open **crash.log** (double-click; it opens in Notepad). No file means no crash.
4. Each line is: date and time (Greek) | version (prod) | what | the error.
   Lines from "watcher" say the app ended and was restarted.
5. To give it to Claude Code: AnyDesk **File transfer** → copy `crash.log` to
   your laptop → drag it into the Claude Code chat. It contains no names,
   codes or passwords.

If the message «Clock In: έκλεισε» is on screen: click **OK**, start Clock In
from the Start menu, then send `crash.log` to Claude Code.

## 8. Check the phone remotely: «Διαγνωστικά»

One screen with everything about the phone. No passwords on it.

1. Ask your dad to open Clock In → **☰** (top right) → **«Διαγνωστικά»**.
2. Ask him to take a screenshot (on the Redmi: **Volume down + Power**
   together, or swipe down with three fingers) and send it to you on Viber.
3. What to look at:
   - **Εφαρμογή:** the version, and «(κανονική)». «(δοκιμαστική)» means the
     wrong APK was installed.
   - **Android System WebView:** 91 or higher (red if too old, section 10).
   - **Τρόπος ειδοποίησης:** «Χτύπημα» or «Ειδοποίηση».
   - Every setting ✓, or «δεν υπάρχει σε αυτή την έκδοση Android» (normal on Android 11).
   - **Επιπλέον ρύθμιση κατασκευαστή:** «σημειώθηκε «Το έκανα»».
   - **Τελευταίος έλεγχος στο παρασκήνιο:** within the last half hour and
     «επιτυχής». Hours old means MIUI is stopping the app (section 9).
   - **Επόμενη ειδοποίηση:** the next shift time.
   - **Τελευταία ειδοποίηση:** «άνοιξε σε πλήρη οθόνη» is perfect; «μόνο ως
     ειδοποίηση» means a MIUI pop-up setting is missing (section 9, step 2).
   - **Τελευταίο απρόσμενο κλείσιμο:** «κανένα» is normal. Red text means the
     app crashed in this version: send the screenshot to Claude Code.

## 9. The phone's alarms stopped: the MIUI settings

MIUI stops apps in the background unless told not to. If alarms come late or
not at all, go through these with your dad (or yourself over AnyDesk). The
words may differ a little between MIUI versions; English in brackets.

1. **In Clock In:** ☰ → «Ρυθμίσεις τηλεφώνου για τις ειδοποιήσεις». Every
   item with ✗ → **«Ρύθμιση»** → follow the text on screen → come back.
2. **The four Xiaomi steps** (same screen, «Επιπλέον ρυθμίσεις για Xiaomi»;
   each «Άνοιγμα» button opens the right MIUI page):
   1. **Αυτόματη εκκίνηση (Autostart):** Clock In switched **on**.
   2. **Άλλα δικαιώματα (Other permissions):** **«Εμφάνιση αναδυόμενων
      παραθύρων κατά την εκτέλεση στο παρασκήνιο»** (Display pop-up windows
      while running in the background) → **Αποδοχή (Accept)**, and
      **«Εμφάνιση στην οθόνη κλειδώματος»** (Show on Lock screen) →
      **Αποδοχή**.
   3. **Εξοικονόμηση μπαταρίας (Battery saver):** **«Χωρίς περιορισμούς»**
      (No restrictions).
   4. **Lock in Recents:** press the square button (or swipe up and hold) to
      see recent apps → press and hold the Clock In card → tap the
      **lock 🔒**. A small lock appears on the card.
   Then **«Το έκανα»**.
3. **Notifications:** phone Settings → **Ειδοποιήσεις (Notifications)** →
   Clock In → allowed; the category «Χτύπημα για την κάρτα» on; **Lock screen
   notifications** allowed.
4. **Phone battery saver / Ultra battery saver** should be off. (Clock In's
   alarms still ring with battery saver on, but MIUI's ultra mode stops
   most apps.)
5. Check with «Διαγνωστικά» (section 8), then do a test alarm: add a test
   person 3 minutes from now on the shop PC, lock the phone, wait → the alarm
   screen appears → «Σταμάτημα» → delete the test person.

## 10. Update Android System WebView

Clock In's screens need **Android System WebView 91 or newer**. With an older
one, the app shows a Greek warning with a button to the Play Store each time
it opens, and «Διαγνωστικά» shows the version in red.

1. On the phone: **Play Store** → search **Android System WebView**.
   (Or the warning's **«Άνοιγμα Play Store»** button.)
2. Tap **Update / Ενημέρωση**. If it only shows **Open** or **Uninstall**, it
   is already up to date.
3. If the Play Store says the app is disabled: phone Settings → **Εφαρμογές
   (Apps) → Διαχείριση εφαρμογών (Manage apps)** → **⋮ → Εμφάνιση όλων /
   system apps** → **Android System WebView** → **Ενεργοποίηση (Enable)**, then
   step 2 again.
4. Restart Clock In (swipe it away in Recents and open it again; if you
   locked it in Recents, unlock first and lock it again after).
5. Check «Διαγνωστικά» → Android System WebView: the version is 91 or higher.

## 11. A Supabase project is paused

Free Supabase projects pause after about 7 days without activity. The shop PC
talks to **prod** every 5 seconds, so prod should never pause. **dev** may
pause between sessions.

Signs: the apps show «Χωρίς σύνδεση» for a long time although the internet
works, or Claude Code says the dev project is unreachable.

1. supabase.com → sign in → you see the project list.
2. A paused project says **Paused**. Open it → **Restore project** → confirm.
3. Wait until it says it is healthy (a few minutes). The apps reconnect by
   themselves, and marks made in the meantime are sent.

Alarms keep ringing while a project is paused (they ring from the last known
schedule).

## 12. The signing key is lost or broken

The phone app is signed with the key in `C:\dev\clock-in-keys\`
(`clock-in.jks` and `keystore.properties`). Every update must be signed with
the **same** key, or the phone refuses to install it over the old app.

**If the laptop died or the folder is gone:**

1. Get the backup (password manager attachment, USB stick or cloud drive).
2. Copy both files back into `C:\dev\clock-in-keys\` (create the folder).
3. Tell Claude Code "the keystore is restored"; it checks that a test build
   is signed with the same certificate as before (SHA-256 starts `fb:a5:4a:7e`).

**If no copy exists at all** (the key is gone for good):

1. Tell Claude Code; it makes a new key (back it up twice right away, as in
   SETUP §6).
2. On your dad's phone: **uninstall** Clock In (long-press the icon →
   Uninstall), then install the new APK and pair it again (section 3) and
   redo the checklist (section 9). The schedules and marks are on the server
   and come back by themselves.
3. Android developer verification (section 13): a package registered with the
   lost key can't be moved to the new one ("If you lose your signing key you
   won't be able to register your packages", Google's FAQ). Claude Code may
   have to give the app a new package name, registered with the new key.
   Then the steps above still apply.

## 13. Android developer verification (2027)

**What it is.** Google is making Android refuse to install apps whose
developer and package name aren't registered with Google, on phones with
Google services (certified devices, Android 7 or newer). Apps that aren't
registered can then only be installed with a cable (ADB) or after a one-time
"advanced flow" on the phone (developer options, a 1-day wait, warnings).
Updates of unregistered apps fail the same way.

**When, for Greece** (checked on 9 October 2026 on developer.android.com):

- Since 30 September 2026 it applies only in Brazil, Indonesia, Singapore and
  Thailand, and there only to installs from participating app stores. A file
  sent over Viber isn't affected yet anywhere.
- "2027 and beyond": global, including Greece. **Google hasn't given a date.**
- Google recommends registering before the global rollout begins. Do it in
  the coming months, not at the last minute.

**Which account.** Two kinds, both in the **Android Developer Console**:

| | Limited distribution | Full distribution |
|---|---|---|
| Cost | free | US$25 once |
| ID | no government ID | government ID |
| Who installs | at most **20 phones**, each authorised one by one (a QR code or link opened on the phone, with consent on the phone) | anyone with the APK file |
| Meant for | "students and hobbyists", sharing with family and friends, no commercial intent | everyone |

Clock In needs only 2 phones (your dad's and your test phone), so limited
distribution fits the device limit. But each phone must do the QR-code
authorisation, done remotely with your dad, and Google describes it for
non-commercial use; the app isn't sold, but it is used by a business. **My
recommendation: full distribution** (US$25 + ID): no per-phone step for your
dad, and no doubt about "commercial intent". Limited distribution is the free
alternative.

**Steps** (the console's wording may change; Google's guide:
developer.android.com/developer-verification/guides/android-developer-console):

1. Use a Google account with **2-Step Verification** on
   (myaccount.google.com → Security).
2. Go to the Android Developer Console (linked from
   developer.android.com/developer-verification) → sign in → create an
   account → choose **Full distribution** (or **Limited distribution**).
   - Full: pay US$25, upload your ID, wait for the identity check (Google
     emails you).
   - Limited: link a Google payments profile (legal name and address; no
     fee) and give a contact e-mail.
3. **Packages** page → **register a package**:
   - Package name: `io.github.minasskasss.clockin`
   - SHA-256 certificate fingerprint of the signing key:
     `FB:A5:4A:7E:6D:C4:26:78:6B:C8:15:91:05:CB:D2:10:3B:D6:B6:13:9B:14:C0:72:1A:89:C0:F3:50:3C:F5:5D`
     (Claude Code can print it again with `apksigner`.)
   - The status becomes **In review**.
4. **Prove you own the key**: the console gives a small file ("snippet") to
   put into an APK. Download it, give it to Claude Code (drag it into the
   chat; it is not secret) and say "build the verification APK". It builds an
   APK signed with the Clock In key that contains the file. Upload that APK
   in the console. Don't install it anywhere.
5. Wait for Google's e-mail: the package status becomes **Registered**.
6. Limited distribution only: for each phone, the console makes a QR code or
   link → open it on that phone → accept. (Your dad's phone and your S24.)
7. Every release in 2027 (and before go-live of any new phone): ask Claude
   Code to re-check developer.android.com/developer-verification, because the
   rules are still changing.

## 14. Set up a new laptop for building Clock In

Only needed if your laptop is replaced. Follow SETUP §2 (Git, GitHub CLI, C++
build tools, Rust, Node.js, pnpm, Claude desktop app), §3C (local sessions)
and §6 (Android Studio). Then:

1. **Get the code:**

   ```powershell
   mkdir C:\dev
   cd C:\dev
   gh repo clone minasskasss/clock-in
   cd clock-in
   pnpm install
   ```

2. **Strawberry Perl** (the Windows build of the encrypted database needs it):
   `winget install --id StrawberryPerl.StrawberryPerl -e`, then open a new
   PowerShell window and check `perl -v` says "Strawberry".
3. **Restore the keystore** into `C:\dev\clock-in-keys\` (section 12).
4. **Environment variables** for Android builds: Start → type "environment" →
   **Edit environment variables for your account** → under "User variables"
   **New…** for each (change the user name and NDK folder to what this laptop has):

   | Name | Value |
   |---|---|
   | `JAVA_HOME` | `C:\Program Files\Android\Android Studio\jbr` |
   | `ANDROID_HOME` | `C:\Users\<you>\AppData\Local\Android\Sdk` |
   | `NDK_HOME` | `C:\Users\<you>\AppData\Local\Android\Sdk\ndk\<version>` (the folder name inside `Sdk\ndk`) |

   → **OK** → **OK**. Then quit the Claude desktop app completely (tray icon →
   Quit) and open it again.
5. **MSYS2 with Perl and make** (cross-compiling for Android), in PowerShell:

   ```powershell
   winget install --id MSYS2.MSYS2 -e
   C:\msys64\usr\bin\bash.exe -lc "pacman -Syu --noconfirm"
   C:\msys64\usr\bin\bash.exe -lc "pacman -Syu --noconfirm"
   C:\msys64\usr\bin\bash.exe -lc "pacman -S --needed --noconfirm perl make"
   C:\msys64\usr\bin\make.exe --version; C:\msys64\usr\bin\perl.exe -v
   ```

   Keep the default folder `C:\msys64`; click **Yes** if Windows asks. The
   update line is run twice on purpose. Don't add MSYS2 to the Windows PATH.
6. **The `.env` files** (public config, not secret): create
   `C:\dev\clock-in\.env.dev` and `.env.prod` with the project URL and
   publishable key of each project (Supabase → project → **Connect** / Project
   Settings → **API Keys**), in the format of `.env.example`. Or paste the
   values into the Claude Code chat and ask it to create them.
7. **Database connection strings** (secret) in the Claude desktop app's local
   environment editor: `CLOCKIN_DEV_DB_URL` and `CLOCKIN_PROD_DB_URL`, as in
   SETUP §4 and §5.
8. Start a Claude Code session in `C:\dev\clock-in` and ask it to check every
   tool's version.

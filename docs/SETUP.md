# Clock In — Setup guide for Minas

This is your guide. Claude Code reads it too, so it knows what you've done.
Sections 1–3B happen before Phase 0, and 3C before Phase 2. Sections 4–8 happen when Claude Code stops and asks for them.

**Never paste these into any chat** (Claude Code or Claude chat):

- the admin passphrase;
- the quit code;
- the Supabase database passwords;
- the connection strings.

The project URL and publishable key are fine to paste; they're public by design.

---

## 1. Accounts you need

| Account | When | Notes |
|---|---|---|
| GitHub | now | The repo `clock-in` will be private |
| Claude plan with Claude Code | now | Used through the Claude desktop app |
| Password manager | now | For the passphrase, DB passwords and keystore passwords |
| Supabase | Phase 2 | Sign in with GitHub. Free plan |
| AnyDesk | go-live | On your laptop, on the shop PC, and optionally on your dad's phone |

---

## 2. Install the tools on your laptop

You've built Tauri apps before, so most of this may already be installed. Check first. Open **PowerShell** and run:

```powershell
git --version; gh --version; rustc --version; cargo --version; node --version; pnpm --version
```

Install only what's missing. After each install, **open a new PowerShell window** so it sees the new tool.

| Tool | Install |
|---|---|
| Git | `winget install --id Git.Git -e`, then `git config --global user.name "Your Name"` and `git config --global user.email "your-github-email"` |
| GitHub CLI | `winget install --id GitHub.cli -e`, then `gh auth login` → GitHub.com → HTTPS → Yes (authenticate Git) → Login with a web browser |
| C++ build tools (needed by Rust) | visualstudio.microsoft.com/downloads → **Build Tools for Visual Studio** → tick **Desktop development with C++** → Install |
| Rust | `winget install --id Rustlang.Rustup -e`, then in a new window `rustup default stable` |
| Node.js | `winget install --id OpenJS.NodeJS.LTS -e` |
| pnpm | `npm install -g pnpm` |
| Claude desktop app | claude.com/download → install → sign in → click the **Code** tab |

WebView2 is already part of Windows 10/11.

Run the check command again: every tool should print a version.
If you installed anything while the Claude desktop app was open, **quit it completely** (tray icon → Quit) and reopen it, so it picks up the new tools.

---

## 3. Project, GitHub and sessions

Phases 0–1 run in a **cloud session**: Anthropic's computer, which keeps working while your laptop is off. Phases 2–6 run in a **local session** on your laptop. The later phases need things only your laptop has: the Supabase database connection, the Windows app actually running, and the Android phone over USB.

### 3A. Put the docs on GitHub (once, before Phase 0)

A cloud session starts from what's on GitHub, not from your laptop, so the docs must be pushed first.

1. Your files should already be in place:
   - `C:\dev\clock-in\CLAUDE.md`
   - `C:\dev\clock-in\docs\` (SPEC, ARCHITECTURE, PLAN, DECISIONS, SETUP)
   - `C:\dev\clock-in\tools\generate-passphrase.ps1`
2. In PowerShell (replace `YOUR-USERNAME`):

   ```powershell
   cd C:\dev\clock-in
   git init -b main
   git add .
   git commit -m "docs: planning documents"
   git remote add origin https://github.com/YOUR-USERNAME/clock-in.git
   git pull origin main --allow-unrelated-histories --no-edit
   git push -u origin main
   ```

   If the `git pull` line says `couldn't find remote ref main`, your GitHub repo is empty. That's fine: just run the `git push` line.
3. On github.com, open the repo and check that `CLAUDE.md`, `docs/` and `tools/` are there.
4. In the repo → **Settings → General → Pull Requests** → tick **Automatically delete head branches**. Cloud sessions aren't allowed to delete branches, so GitHub cleans them up after each merge instead.

### 3B. Cloud sessions (Phases 0 and 1)

**One-time setup:**

1. Go to **claude.ai/code** and follow the onboarding. Authorise the **Claude GitHub App** and give it access to `clock-in`. A cloud environment called **Default** is created.
2. Configure the environment:
   - At claude.ai/code, click the **cloud icon** above the message box → **Cloud** → hover **Default** → **gear** icon.
   - **Network access:** **Custom**. In **Allowed domains**, enter `eff.org` and `www.eff.org` (one per line). Tick **Also include default list of common package managers**.
   - **Environment variables:** leave **empty**. Anyone using an environment can read these values, so nothing secret goes here, ever.
   - **Setup script:** paste the script below, then save.

   ```bash
   #!/bin/bash
   # Tauri 2 Linux build dependencies (so the app compiles on the cloud VM)
   apt-get update
   apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
     libayatana-appindicator3-dev libxdo-dev libssl-dev build-essential curl wget file || true
   rustup component add clippy rustfmt || true
   exit 0
   ```

**Start a phase:**

1. In the Claude desktop app → **Code** tab → **+ New session**:
   - **Environment:** **Cloud** (pick the environment you configured)
   - **Repository:** `clock-in`
   - **Permission mode:** **Auto**
   - (You can also do this from claude.ai/code or the Claude mobile app.)
2. Type **`Start Phase 0`** (later: `Start Phase 1`).
   - Claude Code knows the repo already exists; it's in CLAUDE.md and PLAN.md.
   - It keeps working if you close the app or your laptop. Check on it from the desktop app, claude.ai/code or your phone.

**Testing a cloud branch** on your laptop. In PowerShell, using the branch name from the report:

```powershell
cd C:\dev\clock-in
git fetch origin
git checkout phase-0-scaffold
pnpm install
pnpm tauri dev
```

When you're done testing, run `git checkout main`.

### 3C. Switching to local sessions (before Phase 2)

1. Make sure Phase 1 is merged. Then in PowerShell:

   ```powershell
   cd C:\dev\clock-in
   git checkout main
   git pull
   ```

2. In the Claude desktop app → **Code** tab → **+ New session** (Ctrl+N):
   - **Environment:** **Local**
   - **Project folder:** `C:\dev\clock-in`
   - **Worktree:** leave it **off**. You run one session at a time, and the branch must be in the main folder for testing.
   - **Model:** the strongest model on your plan
   - **Permission mode:** **Auto**. If you don't see it, update the app (Help → Check for Updates); **Accept edits** works too, with more prompts.
3. Type **`Start Phase 2`**. It will soon stop for SETUP §4 (Supabase).

**If you want to steer local work from your phone while you're out:** Claude Code has a **Remote Control** feature that connects the mobile app to a session running on your laptop. The laptop must stay on and awake.

### The loop for every phase (cloud or local)

1. Claude Code works until it opens a PR and stops with a report: Done / Needs you / How to test / Next.
2. If "Needs you" lists steps, do them. They match a section in this guide.
3. Test with the checklist:
   - **cloud phases:** use "Testing a cloud branch" above;
   - **local phases:** open the terminal pane with **Ctrl+\`** and run any command it gives you.
4. **If something is wrong,** tell it in the same session: what you did, what you expected, what happened. Drag in a screenshot if useful. It fixes and re-reports.
5. **When everything passes,** write **`approved, merge`**. It merges; if a cloud session can't merge, it will ask you to click **Squash and merge** on the PR page.
6. Start a **new session** for the next phase.

**Tips:**

- If you hit a rate limit, wait, then write `Continue` in the same session.
- In the PR's CI status bar, leave **Auto-merge off**. Auto-fix can stay on.
- If you want a second opinion before approving, paste the report into the "Clock In" chat project.

---

## 4. Supabase dev project (Phase 2 start)

1. Go to supabase.com → **Sign in** with GitHub. If asked, create an organization: personal, **Free** plan.
   - The free plan allows **2 active projects**, which is exactly dev + prod. If you already have other active free projects, pause one first.
2. Click **New project**:
   - **Name:** `clockin-dev`
   - **Database password:** click **Generate**, then save it in your password manager.
     - Use letters and numbers only. If it contains symbols like `@ # / % :`, generate another one, because symbols break connection strings.
   - **Region:** Central EU (**Frankfurt**)
   - Leave the Data API settings at their defaults.
   - Click **Create** and wait until the project is ready.
3. **URL and key (safe to paste):**
   - Project Settings → **API Keys** → copy the **Publishable key** (starts with `sb_publishable_`).
   - The **Project URL** is `https://<project-ref>.supabase.co`; it's shown in the **Connect** dialog or the project overview.
   - Paste both into the Claude Code chat.
4. **Connection string (secret):**
   - Click **Connect** (top bar) → **Connection string** → method **Session pooler** → copy the URI.
   - Replace `[YOUR-PASSWORD]` with the database password. Do this in Notepad, not in a chat.
5. **Store it for Claude Code:**
   - In the Claude desktop app, click the environment dropdown in the prompt box → hover **Local** → click the **gear** icon.
   - Add the variable **`CLOCKIN_DEV_DB_URL`** with the full URI as its value → **Save**.
   - Then close Notepad without saving.
6. Write `Supabase dev is ready` in the session. If Claude Code says it can't see the variable, start a new session and write `Continue Phase 2`.

**Note:** free projects **pause after about a week without activity**. The dev project may pause between phases. If Claude Code reports it unreachable, open the dashboard and click **Restore / Resume project**.

---

## 5. Supabase prod project (Phase 4 end)

Same as section 4, with these differences:

- **Name:** `clockin-prod`, with a **different** database password, saved in the password manager.
- The variable is **`CLOCKIN_PROD_DB_URL`**.
- Paste the prod URL and publishable key into the chat.

The shop PC polls prod all day, so prod won't pause.

---

## 6. Android tools and test phone (Phase 5 start)

1. Install Android Studio: `winget install --id Google.AndroidStudio -e` (or developer.android.com/studio). Open it once and finish the setup wizard with **Standard**.
2. Welcome screen → **More Actions → SDK Manager**:
   - **SDK Platforms:** the newest Android version.
   - **SDK Tools:** Android SDK Build-Tools, **NDK (Side by side)**, Android SDK Command-line Tools (latest), Android SDK Platform-Tools.
   - Click **Apply** and wait.
3. **Environment variables:** Claude Code gives you the exact values. On this laptop (Phase 5):
   - `JAVA_HOME` = `C:\Program Files\Android\Android Studio\jbr`
   - `ANDROID_HOME` = `C:\Users\minas\AppData\Local\Android\Sdk`
   - `NDK_HOME` = `C:\Users\minas\AppData\Local\Android\Sdk\ndk\30.0.16248370` (change the last folder if a newer NDK is installed)
   - Set them via Start → "Edit environment variables for your account" → New.
   - Then quit the Claude desktop app completely and reopen it.
4. **MSYS2 with Perl and make.** The Android build needs them; `tools\android-env.ps1` sets up the rest for each build.
   - On this laptop they are already there (MSYS2 in `C:\msys64`; Claude Code added `make` in Phase 5). On a new laptop:
     1. In **PowerShell**: `winget install --id MSYS2.MSYS2 -e`. Keep the default folder `C:\msys64`. Click **Yes** if Windows asks for permission.
     2. Still in PowerShell, update MSYS2. Run this **twice**, because the first run may only update MSYS2 itself:
        ```powershell
        C:\msys64\usr\bin\bash.exe -lc "pacman -Syu --noconfirm"
        ```
     3. Install Perl and make:
        ```powershell
        C:\msys64\usr\bin\bash.exe -lc "pacman -S --needed --noconfirm perl make"
        ```
     4. Check: `C:\msys64\usr\bin\make.exe --version; C:\msys64\usr\bin\perl.exe -v`. Both print a version.
   - Don't add `C:\msys64\usr\bin` to the Windows PATH; the build scripts use it only for Android builds.
5. **Test phone** (your own Android phone, or any spare one):
   - Settings → About phone → tap **Build number** 7 times;
   - Developer options → turn on **USB debugging**;
   - connect it by USB and tap **Allow** on the phone.
   - No Android phone? Tell Claude Code; it will set up the emulator instead. The brand-specific checks then happen on your dad's phone at go-live.
6. **End of Phase 5:** Claude Code creates the signing keystore in `C:\dev\clock-in-keys\`: `clock-in.jks` (the key, alias `clock-in`) and `keystore.properties` (its path, alias and password; the store and key passwords are the same).
   - Copy that folder and the keystore passwords to **two** places: your password manager, plus a USB stick or private cloud drive.
   - Builds read the key only from that folder. GitHub's checks build only a debug app (signed with a throwaway debug key) and never see it.
   - Without this key, future updates would force your dad to uninstall, reinstall and re-pair.

---

## 7. Go-live: shop PC in Rhodes, over AnyDesk (after Phase 4 is merged)

### Before you connect

1. **Generate the passphrase** on your laptop. In PowerShell:

   ```powershell
   cd C:\dev\clock-in
   powershell -ExecutionPolicy Bypass -File .\tools\generate-passphrase.ps1 -WordList .\assets\eff_large_wordlist.txt
   ```

   Save the result in your password manager. You're the recovery person, so you keep a copy.
2. **Pick a 4-digit quit code.** It isn't secret; it only stops accidental closing.
3. **Phone your dad.**
   - Read him the passphrase slowly, by voice; don't text it.
   - He writes it on paper and keeps it **away from the shop** (wallet or home).
   - Tell him the quit code too.

### On the shop PC (AnyDesk)

4. Send the installer (`Clock In_x.y.z_x64-setup.exe`) with AnyDesk's file transfer and run it. If SmartScreen appears: **More info → Run anyway**.
5. The first-run screen appears. Choose **"Set up as first device"**, then enter the passphrase twice and the quit code twice.
6. Settings → add every staff member and their hours, with your dad on the phone.
7. **Windows settings on the shop PC:**
   - **Power:** sleep → **Never** when plugged in. The screen may turn off; that's fine.
   - **Sound:** speakers connected and on, volume high.
   - **Windows Update → Active hours:** cover the opening hours, so Windows never restarts mid-shift.
   - **Login:** the app starts when the Windows user logs in. After any restart, someone must log in, or you can turn on automatic sign-in.
   - **AnyDesk:** set up **unattended access** with a strong password, so you can install updates without anyone at the PC.
8. **Test:** add a test person with a block starting 3 minutes from now → the alarm rings → Stop → delete the test person.

**Updates later:** send the new installer the same way and run it. It replaces the old version and keeps data and pairing.

---

## 8. Go-live: your dad's phone, remotely (during Phase 6, with the prod APK)

1. **Optional but helpful: AnyDesk on his phone.**
   - He installs **AnyDesk** from the Play Store, opens it, and reads you his address. You connect and he accepts.
   - You'll always be able to **see** his screen. **Control** works on many phones, but some ask for an extra AnyDesk plugin or accessibility permission.
   - If control doesn't work, guide him by phone while watching his screen.
2. **Send him the APK file** (`clock-in-x.y.z.apk`) via Viber or WhatsApp as a file, or as a Google Drive link.
3. **On his phone:**
   - Tap the file. Android asks to allow installs from that app → **Allow from this source** → go back → **Install**.
   - If **Google Play Protect** warns, tap **More details → Install anyway**. Exact wording varies by brand.
4. Open **Clock In** → **"Pair this device"** → he types the passphrase from his paper (or you type it over AnyDesk).
5. **Onboarding checklist:** follow it until everything shows ✓.
   - Battery must be **Unrestricted / Not optimised**.
   - On Xiaomi or Samsung, also do the extra step the app shows (autostart / "never sleeping apps").
6. Choose **Ring** or **Notification**, whichever he prefers.
7. **Test:** add a test person 3 minutes from now → both the shop PC and the phone alert → Stop on each → delete the test person.

**Updates later:** send the new APK the same way → tap → **Update**. **Never uninstall first**; that loses pairing.

**Before 2027:** Google's developer verification for sideloaded apps reaches Greece in 2027. RECOVERY.md (Phase 6) lists the steps: a free Android Developer Console registration for small, personal distribution. Do it before then, or new installs may be blocked.

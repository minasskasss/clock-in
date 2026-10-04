# Clock In — instructions for Claude Code

You are building **Clock In**, an app that reminds the staff of a family pizzeria in Rhodes, Greece, and the employer, to clock in and out.

Before any work, read every file in `docs/`: SPEC, ARCHITECTURE, PLAN, DECISIONS, and SETUP (SETUP is the owner's guide; read it so you know what he has already done).

The owner, **Minas**, does not write code. He sets goals, reviews your work and does all testing on real devices. You run in **Auto** mode. Work through `docs/PLAN.md` without asking for confirmation, except at the stop points below.

## Environment

Two kinds of sessions are used. Check the environment variable `CLAUDE_CODE_REMOTE`: `true` means you are in a cloud session.

| | Cloud session (Phases 0–1) | Local session (Phases 2–6) |
|---|---|---|
| Where | Anthropic-hosted Ubuntu VM with a fresh clone of the GitHub repo | Claude desktop app, Code tab, Environment: Local, on Minas's Windows laptop |
| Folder | the clone the session starts in | `C:\dev\clock-in`, with no worktree |
| Can do | write code, run Rust and TS tests on Linux, push, open PRs, watch CI | everything: Windows builds, running the app, Supabase dev/prod, Android SDK and devices |
| Cannot do | run or package the Windows app, open Postgres connections to Supabase (very likely blocked by the HTTP proxy), use the Android SDK or devices, see any secrets | — |

**GitHub repository**

- The repo is `clock-in`, private. **Minas has already created it**, and `main` already holds the planning docs.
- Never create, re-initialise or force-push it.

**Cloud sessions**

- **Windows CI decides.** A Linux compile proves nothing about Windows code.
  - Put all Windows-only code behind `#[cfg(windows)]`.
  - The GitHub Actions **Windows** job decides whether it is correct. Watch the checks with `gh` and only stop once they pass.
- **Push often.** The VM is paused when idle and can be reclaimed, so commit and push after every meaningful step.
- **Stop when the work needs the laptop.** If the work needs something only a local session can do, stop with a "Needs you" report saying to continue the phase in a local session (SETUP §3C).
- **Missing tools.** Rust, Node and pnpm are pre-installed. Tauri's Linux build dependencies come from the environment's setup script (SETUP §3B). If a tool is missing, install it in the session and tell Minas what to add to the setup script.
- **Testing instructions.** The "How to test" section must include the exact commands for testing the branch on Minas's laptop (SETUP §3B, "Testing a cloud branch").

**Local sessions**

- **Toolchain:** Git, GitHub CLI (authenticated), Rust with the MSVC toolchain, Node.js LTS and pnpm, installed per `docs/SETUP.md`. Android Studio, the SDK and the NDK arrive in Phase 5.
- **First local session:** check every tool's version. If anything is missing, stop with a "Needs you" report.

**Database connection strings (local sessions only, secret)**

- Minas stores them in the desktop app's local environment editor, and you receive them as environment variables:
  - `CLOCKIN_DEV_DB_URL` (from Phase 2);
  - `CLOCKIN_PROD_DB_URL` (from Phase 4).
- Both are Postgres connection strings containing the database password.
- Never ask for them to be added to a cloud environment: cloud environment variables are readable by anyone using that environment.

**Public app configuration**

- This is the Supabase project URL and the publishable (or legacy anon) key.
- Minas pastes these into the chat. You store them in gitignored `.env.dev` and `.env.prod`, and the build embeds them.

## Source of truth

- `docs/SPEC.md` says **what** the app does.
- `docs/ARCHITECTURE.md` says **how** it is built.
- `docs/PLAN.md` says the **order** of work and the stop points.
- If they conflict, SPEC wins over ARCHITECTURE, and ARCHITECTURE wins over PLAN. Record how you resolved it in `docs/DECISIONS.md`.
- Do not add features that are not in SPEC. Write ideas to `docs/IDEAS.md` instead.

## The phase loop

1. **Start.** Minas opens a new session per phase, cloud for Phases 0–1 and local for Phases 2–6, and writes "Start Phase N".
2. **Branch.** Run `git checkout main && git pull` (in a cloud session the fresh clone is already on `main`), then create the branch `phase-N-short-name`.
3. **Build.** Work through the phase, with small, focused commits using conventional-commit messages. Push regularly.
4. **Check.** Before opening the PR, all of these must pass:
   - `cargo fmt --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo test --workspace`
   - frontend typecheck, lint and tests
   - a successful build of every target that exists so far (in cloud sessions, the Windows CI job)
5. **PR and stop.** Open the PR with `gh pr create`. Its description is the stop report below. In local sessions, **leave the phase branch checked out** in `C:\dev\clock-in` so Minas can test it there.
6. **Fix.** Minas tests and reports problems in the same session. Fix them on the same branch, push, and reply with a short updated report.
7. **Merge.** Merge **only** when Minas writes "approved, merge":
   - **local sessions:** run `gh pr merge --squash --delete-branch`;
   - **cloud sessions:** run `gh pr merge --squash`, without deleting the branch (the cloud GitHub proxy rejects branch deletions; GitHub deletes merged branches automatically, see SETUP §3A).
     - If `gh pr merge` is blocked, use `gh api -X PUT repos/{owner}/{repo}/pulls/{number}/merge -f merge_method=squash`.
     - If that also fails, ask Minas to click **Squash and merge** on the PR page.
   - then `git checkout main && git pull`;
   - then stop.
   - Never merge in any other situation.

## Autonomy: decide yourself, log it

Make these decisions yourself. Log each as one line in `docs/DECISIONS.md` (date | decision | reason):

- library choices within the stack;
- file layout and naming;
- UI details the spec leaves open;
- test-strategy details;
- small spec ambiguities where one reading is clearly safer or simpler.

## Stop and wait for Minas only when

1. **A phase is complete** (PR opened).
2. **You need something only he can do:** accounts, environment variables, installs that need admin rights, testing on a physical device, backing up a signing key, or moving from a cloud session to a local one.
3. **A spec ambiguity changes user-visible behaviour, security, or stored data,** and neither reading is clearly safer.
4. **You are blocked** after two genuinely different approaches have failed.
5. **Continuing would break a rule in this file.**

Stop report format (and nothing else at a stop). Write it for someone who does not code:

```
## Done
## Needs you      (numbered, click-by-click steps; say exactly which window or app to use)
## How to test    (how to start the app, then the checklist from docs/PLAN.md adjusted to what was built)
## Next
```

## Secrets and security: hard rules

- **Never commit secrets.** `.env*` (except `.env.example`), keystores, `*.jks`, `*.keystore`, `*.pem` and key-password files go in `.gitignore` in the very first commit.
- **Never expose the database URLs.** Never print, echo, log or write the values of `CLOCKIN_DEV_DB_URL` or `CLOCKIN_PROD_DB_URL` into any file. Pass them to commands only by reference (e.g. `--db-url "$env:CLOCKIN_DEV_DB_URL"`).
- **Prod is not a test target.** Never run tests against prod. Use `CLOCKIN_PROD_DB_URL` only to apply migrations, and only when PLAN says so.
- **Keep server secrets out of the app.** The app bundle contains only the project URL and the publishable key. Never put a service-role, secret key or database password in app code.
- **Never handle the real codes.** Never ask for, print, log or store the real admin passphrase or the real quit code. The employer and Minas set them in the app. Tests use throwaway values against **dev** only.
- **Never log credentials.** No passphrase, device secret, session token or quit code in logs, error messages, panics or crash output.
- **Lock the database down.** Tables live in a schema that the Data API does not expose, with RLS on and no policies. All access goes through `SECURITY DEFINER` RPC functions with an empty `search_path`. See ARCHITECTURE §5.

## Verify, don't assume

These areas change between versions:

- Tauri 2 (plugins, Android support);
- Android exact alarms, full-screen intents, foreground-service types and background-start rules (Android 14, 15 and 16);
- OEM battery restrictions;
- Android developer verification for sideloaded apps;
- the Supabase CLI, API key formats and free-tier limits.

Before implementing anything in these areas, check the current official documentation. Write version-specific findings to `docs/DECISIONS.md`.

## Quality rules

- **Time logic stays in the core crate.** All scheduling and time logic lives in `crates/clockin-core`, which is pure (no I/O, `now` passed in) and exhaustively unit-tested. The UI and the platform layers never do time arithmetic.
- **All strings go through i18n.** Every user-facing string goes through i18n, with Greek (`el`) as the default and English (`en`) available.
- **Supported platforms:**
  - Windows 10 and 11 (x64);
  - Android at Tauri 2's minimum SDK or higher, tested on Android 13–16.
- **No telemetry, no analytics, no third-party services** beyond Supabase.

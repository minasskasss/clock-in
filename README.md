# Clock In

Reminds the staff of a family pizzeria in Rhodes, and the employer, to clock
in and out with the Greek digital work card (ψηφιακή κάρτα εργασίας). It is
only a reminder: not the legal record, and not connected to ERGANI.

- **Shop PC (Windows 10/11):** the Today list of who works when, and an
  always-on-top alarm with sound at each check-in and check-out time.
- **Employer's phone (Android 8+):** the same list, and a full-screen alarm or
  a notification, also with the app closed and the phone locked.
- **Server:** one Supabase Postgres project (EU, Frankfurt), reached only
  through locked-down RPC functions.

The app is in Greek only.

## Documents

| | |
|---|---|
| [docs/SPEC.md](docs/SPEC.md) | what the app does |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | how it is built, security |
| [docs/PLAN.md](docs/PLAN.md) | build phases and checklists |
| [docs/DECISIONS.md](docs/DECISIONS.md) | every decision, with its reason |
| [docs/SETUP.md](docs/SETUP.md) | setting up accounts, the laptop and both devices |
| [docs/RECOVERY.md](docs/RECOVERY.md) | what to do when something goes wrong, and updates |
| [docs/IDEAS.md](docs/IDEAS.md) | ideas not in v1 |
| [CREDITS.md](CREDITS.md) | third-party material |

## Building (Windows laptop)

Tools: see [docs/SETUP.md](docs/SETUP.md) §2 and §6. The Supabase URL and
publishable key go in `.env.dev` / `.env.prod` (format: `.env.example`).

```powershell
pnpm install
pnpm tauri dev                                # development (dev project)
$env:CLOCKIN_ENV = 'dev'; pnpm tauri build    # Windows installer pointed at dev
.\tools\build-apk.ps1 -Env dev                # signed APK pointed at dev
```

Checks run on every pull request: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`,
`pnpm typecheck`, `pnpm lint`, `pnpm test`, and Windows and Android builds.

## Layout

- `crates/clockin-core`: all scheduling and time logic (pure, unit-tested)
- `crates/clockin-sync`: server client, encrypted local database, sync
- `src-tauri`: the app (Rust): commands, alarms, tray, crash log and restart watcher
- `src`: the screens (React + TypeScript); all wording in `src/i18n/el.json`
- `plugins/clockin-alarm`: the Android alarm plugin (Kotlin)
- `supabase/migrations`: database schema and RPC functions

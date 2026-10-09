//! Backend integration tests against the **dev** Supabase project
//! (ARCHITECTURE §12). Most wipe dev's data (devices paired to dev included),
//! so they are `#[ignore]`d and run only on request, one at a time. The ones
//! whose ignore reason says "keeps dev's data" (most in `backend/keep.rs`)
//! only read, or add and remove their own throwaway rows:
//!
//! ```text
//! cargo test -p clockin-sync --test backend -- --ignored --test-threads=1
//! ```
//!
//! - RPCs go over HTTPS with the dev URL and publishable key from `.env.dev`.
//! - `CLOCKIN_DEV_DB_URL` (session pooler) is used to reset state and inspect
//!   tables. Its value is never printed.
//! - They refuse to run if the DB URL equals `CLOCKIN_PROD_DB_URL` or belongs
//!   to a different project than `.env.dev`.
//! - Passphrases and quit codes are throwaway values made up here.

use clockin_core::{
    MarkKind, OverrideKind, alarm_plan, business_date_for, passphrase_from_indices, plan_window,
};
use clockin_sync::{
    Api, ApiError, DeviceSecret, ErrorCode, MarkRequest, OverrideBlockInput, Pairing, Platform,
    QuitCode, ServerConfig, SessionToken, SettingsInput, Store, StoreKey, SyncEngine,
    WeekBlockInput,
};
use jiff::civil::{Date, Time, Weekday};
use jiff::{SignedDuration, Timestamp};
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};
use uuid::Uuid;

/// Phase 6 checks that keep dev's data.
#[path = "backend/keep.rs"]
mod keep;

/// Dev state is shared, so tests take turns even without `--test-threads=1`.
static LOCK: Mutex<()> = Mutex::const_new(());

const QUIT: &str = "4321";

struct Dev {
    api: Api,
    config: ServerConfig,
    db: tokio_postgres::Client,
    _guard: MutexGuard<'static, ()>,
}

fn read_env_dev() -> ServerConfig {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.env.dev");
    let text = std::fs::read_to_string(path).expect(".env.dev is missing at the repo root");
    let get = |name: &str| {
        text.lines()
            .filter_map(|l| l.trim().strip_prefix(name)?.strip_prefix('='))
            .map(|v| v.trim().to_owned())
            .find(|v| !v.is_empty())
            .unwrap_or_else(|| panic!("{name} is empty in .env.dev"))
    };
    ServerConfig {
        url: get("SUPABASE_URL"),
        publishable_key: get("SUPABASE_PUBLISHABLE_KEY"),
    }
}

/// The project ref, from `https://<ref>.supabase.co`.
fn project_ref(config: &ServerConfig) -> String {
    let host = config
        .base_url()
        .strip_prefix("https://")
        .expect("SUPABASE_URL must start with https://");
    host.split('.').next().unwrap_or_default().to_owned()
}

fn tls() -> rustls::ClientConfig {
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        })
        .with_no_client_auth()
}

/// Supabase's database (and pooler) certificates chain to Supabase's own
/// root CA, not a public one. Both "Supabase Root 2021 CA" certificates are
/// pinned in `supabase-root-ca.crt` (fingerprints in the file; they match the
/// roots embedded in the official Supabase CLI), so the connection is fully
/// verified, password included.
fn db_tls() -> rustls::ClientConfig {
    use rustls::pki_types::CertificateDer;
    use rustls::pki_types::pem::PemObject;
    let mut roots = rustls::RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(include_bytes!("supabase-root-ca.crt")) {
        roots.add(cert.expect("bad pinned certificate")).unwrap();
    }
    assert_eq!(roots.len(), 2);
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

async fn connect_db(config: &ServerConfig) -> tokio_postgres::Client {
    let url = std::env::var("CLOCKIN_DEV_DB_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .expect("CLOCKIN_DEV_DB_URL is not set");
    if let Ok(prod) = std::env::var("CLOCKIN_PROD_DB_URL") {
        assert!(
            prod.trim() != url.trim(),
            "refusing to run: CLOCKIN_DEV_DB_URL equals CLOCKIN_PROD_DB_URL"
        );
    }
    let mut pg: tokio_postgres::Config = url
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("CLOCKIN_DEV_DB_URL is not a valid connection string"));
    // Session-pooler user names are `postgres.<project-ref>`.
    let expected_user = format!("postgres.{}", project_ref(config));
    assert!(
        pg.get_user() == Some(expected_user.as_str()),
        "refusing to run: CLOCKIN_DEV_DB_URL is not for the project in .env.dev"
    );
    pg.ssl_mode(tokio_postgres::config::SslMode::Require);
    let tls = tokio_postgres_rustls::MakeRustlsConnect::new(db_tls());
    // On failure, print only the SQLSTATE of a server error (its text can
    // name the database user) or the transport cause (TLS, network). The
    // connection string is never formatted anywhere.
    let (client, connection) = pg.connect(tls).await.unwrap_or_else(|e| {
        match (e.as_db_error(), std::error::Error::source(&e)) {
            (Some(db), _) => panic!(
                "cannot connect to the dev database: server error {}",
                db.code().code()
            ),
            (None, Some(cause)) if !e.is_closed() => {
                panic!("cannot connect to the dev database: {cause}")
            }
            _ => panic!("cannot connect to the dev database"),
        }
    });
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

/// Wipes dev and returns a fresh, uninitialised backend.
async fn dev() -> Dev {
    let guard = LOCK.lock().await;
    let config = read_env_dev();
    let db = connect_db(&config).await;
    db.batch_execute(
        "delete from app.admin_sessions where true;
         delete from app.alarm_plan where true;
         delete from app.marks where true;
         delete from app.overrides where true;
         delete from app.schedule_blocks where true;
         delete from app.staff where true;
         delete from app.devices where true;
         update app.auth_state set passphrase_hash = null, failed_count = 0, locked_until = null where id;
         update app.settings set checkin_offset_min = 0, checkout_offset_min = 0, rollover = '05:00',
                                 autostart = true, quit_code = null where id;
         update app.meta set plan_config_version = config_version, plan_horizon_end = null where id;",
    )
    .await
    .expect("resetting dev failed");
    Dev {
        api: Api::new(&config).unwrap(),
        config,
        db,
        _guard: guard,
    }
}

/// Dev as it is, without wiping anything (read-only checks, and checks that
/// remove only their own throwaway rows).
async fn dev_as_is() -> Dev {
    let guard = LOCK.lock().await;
    let config = read_env_dev();
    let db = connect_db(&config).await;
    Dev {
        api: Api::new(&config).unwrap(),
        config,
        db,
        _guard: guard,
    }
}

/// A throwaway 5-word EFF passphrase.
fn throwaway_passphrase() -> String {
    let bytes = Uuid::new_v4().into_bytes();
    let more = Uuid::new_v4().into_bytes();
    let indices: Vec<usize> = bytes
        .chunks(2)
        .chain(more.chunks(2))
        .take(5)
        .map(|c| usize::from(u16::from_le_bytes([c[0], c[1]])) % clockin_core::EFF_WORD_COUNT)
        .collect();
    passphrase_from_indices(&indices).unwrap()
}

fn code(result: Result<impl std::fmt::Debug, ApiError>) -> ErrorCode {
    match result {
        Err(ApiError::Rejected(r)) => r.error,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

fn retry_after(result: Result<impl std::fmt::Debug, ApiError>) -> Option<u32> {
    match result {
        Err(ApiError::Rejected(r)) => r.retry_after_s,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

fn quit() -> QuitCode {
    QuitCode::new(QUIT.into())
}

fn t(h: i8, m: i8) -> Time {
    Time::constant(h, m, 0, 0)
}

fn week(blocks: &[(Weekday, Time, Time)]) -> Vec<WeekBlockInput> {
    blocks
        .iter()
        .map(|&(weekday, start, end)| WeekBlockInput {
            id: None,
            weekday,
            start,
            end,
        })
        .collect()
}

impl Dev {
    /// Initialises dev with a throwaway passphrase; returns the first device.
    async fn init(&self) -> (Pairing, String) {
        let passphrase = throwaway_passphrase();
        let pairing = self
            .api
            .admin_initialize(&passphrase, &quit(), "Test PC", Platform::Windows)
            .await
            .unwrap();
        (pairing, passphrase)
    }

    async fn login(&self, device: &Pairing, passphrase: &str) -> SessionToken {
        self.api
            .admin_login(&device.device_secret, passphrase)
            .await
            .unwrap()
            .session_token
    }

    async fn i64(&self, sql: &str) -> i64 {
        self.db.query_one(sql, &[]).await.unwrap().get(0)
    }

    async fn text(&self, sql: &str) -> String {
        self.db.query_one(sql, &[]).await.unwrap().get(0)
    }

    async fn boolean(&self, sql: &str) -> bool {
        self.db.query_one(sql, &[]).await.unwrap().get(0)
    }

    async fn exec(&self, sql: &str) {
        self.db.batch_execute(sql).await.unwrap();
    }

    async fn business_today(&self) -> Date {
        let now = Timestamp::now();
        business_date_for(now, t(5, 0))
    }
}

fn random_secret() -> DeviceSecret {
    DeviceSecret::new(format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ))
}

// ---------------------------------------------------------------------------

/// Schema `app` is unreachable through the Data API, and the grants are as
/// ARCHITECTURE §5 describes.
/// Leaves dev empty and uninitialised, ready for a manual "Set up as first
/// device" test. Run on its own:
///
/// ```text
/// cargo test -p clockin-sync --test backend reset_dev -- --ignored
/// ```
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn reset_dev() {
    let dev = dev().await;
    assert_eq!(
        dev.i64("select count(*) from app.auth_state where passphrase_hash is null")
            .await,
        1
    );
}

#[tokio::test]
#[ignore = "uses the dev Supabase project; keeps dev's data"]
async fn app_schema_is_unreachable_and_locked_down() {
    let dev = dev_as_is().await;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let http = reqwest::Client::builder()
        .tls_backend_preconfigured(tls())
        .build()
        .unwrap();
    let base = format!("{}/rest/v1", dev.config.base_url());
    let key = dev.config.publishable_key.as_str();

    let tables = [
        "staff",
        "schedule_blocks",
        "overrides",
        "override_blocks",
        "settings",
        "devices",
        "marks",
        "auth_state",
        "admin_sessions",
        "alarm_plan",
        "meta",
    ];
    for table in tables {
        let plain = http
            .get(format!("{base}/{table}"))
            .header("apikey", key)
            .send()
            .await
            .unwrap();
        assert_eq!(plain.status(), 404, "GET {table}");
        let profiled = http
            .get(format!("{base}/{table}"))
            .header("apikey", key)
            .header("Accept-Profile", "app")
            .send()
            .await
            .unwrap();
        assert_eq!(profiled.status(), 406, "GET app.{table}");
        let body = profiled.text().await.unwrap();
        assert!(body.contains("PGRST106"), "{table}: {body}");
    }
    // Helpers are neither in `public` nor reachable through profile headers.
    for helper in [
        "purge",
        "verify_passphrase",
        "auth_device",
        "apply_week",
        "new_token",
    ] {
        let r = http
            .post(format!("{base}/rpc/{helper}"))
            .header("apikey", key)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 404, "rpc/{helper}");
        let r = http
            .post(format!("{base}/rpc/{helper}"))
            .header("apikey", key)
            .header("Content-Profile", "app")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 406, "app rpc/{helper}");
    }
    // Without the key nothing works at all.
    let r = http
        .post(format!("{base}/rpc/get_version"))
        .json(&serde_json::json!({ "p_secret": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // Catalog checks.
    for role in ["anon", "authenticated"] {
        assert!(
            !dev.boolean(&format!(
                "select has_schema_privilege('{role}', 'app', 'USAGE')"
            ))
            .await,
            "{role} has USAGE on app"
        );
        for table in tables {
            assert!(
                !dev.boolean(&format!(
                    "select has_table_privilege('{role}', 'app.{table}',
                        'SELECT, INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER')"
                ))
                .await,
                "{role} has privileges on app.{table}"
            );
        }
        assert_eq!(
            dev.i64(&format!(
                "select count(*) from pg_proc p join pg_namespace n on n.oid = p.pronamespace
                  where n.nspname = 'app' and has_function_privilege('{role}', p.oid, 'EXECUTE')"
            ))
            .await,
            0,
            "{role} can execute an app helper"
        );
    }
    assert_eq!(
        dev.i64(
            "select count(*) from pg_class c join pg_namespace n on n.oid = c.relnamespace
              where n.nspname = 'app' and c.relkind = 'r' and not c.relrowsecurity"
        )
        .await,
        0,
        "a table in app has RLS off"
    );
    assert_eq!(
        dev.i64("select count(*) from pg_policies where schemaname = 'app'")
            .await,
        0
    );
    // Exactly the 20 RPCs in public; each SECURITY DEFINER with an empty
    // search_path, executable by anon only (not PUBLIC, not authenticated).
    assert_eq!(
        dev.i64(
            "select count(*) from pg_proc p join pg_namespace n on n.oid = p.pronamespace
              where n.nspname = 'public'"
        )
        .await,
        20
    );
    assert_eq!(
        dev.i64(
            "select count(*) from pg_proc p join pg_namespace n on n.oid = p.pronamespace
              where n.nspname = 'public'
                and p.prosecdef
                and p.proconfig @> array['search_path=\"\"']
                and has_function_privilege('anon', p.oid, 'EXECUTE')
                and not has_function_privilege('authenticated', p.oid, 'EXECUTE')
                and not exists (select 1 from aclexplode(p.proacl) a where a.grantee = 0)"
        )
        .await,
        20
    );
    // Helpers in app also run with an empty search_path.
    assert_eq!(
        dev.i64(
            "select count(*) from pg_proc p join pg_namespace n on n.oid = p.pronamespace
              where n.nspname = 'app'
                and not coalesce(p.proconfig @> array['search_path=\"\"'], false)"
        )
        .await,
        0
    );
    // No function in app or public is executable by PUBLIC: each has an
    // explicit ACL (NULL would mean the default, which includes PUBLIC) with
    // no PUBLIC entry. Helpers are executable by no API role (checked above).
    assert_eq!(
        dev.i64(
            "select count(*) from pg_proc p join pg_namespace n on n.oid = p.pronamespace
              where n.nspname in ('app', 'public')
                and (p.proacl is null
                     or exists (select 1 from aclexplode(p.proacl) a where a.grantee = 0))"
        )
        .await,
        0
    );
    // Only admin_initialize and pair_device take no device secret.
    let public_rpcs = |filter: &str| {
        format!(
            "select coalesce(string_agg(p.proname, ',' order by p.proname), '')
               from pg_proc p join pg_namespace n on n.oid = p.pronamespace
              where n.nspname = 'public' and {filter}"
        )
    };
    assert_eq!(
        dev.text(&public_rpcs("coalesce(p.proargnames[1], '') <> 'p_secret'"))
            .await,
        "admin_initialize,pair_device"
    );
    assert_eq!(
        dev.text(&public_rpcs("p.proargnames[2] = 'p_session'"))
            .await,
        [
            "admin_logout,change_passphrase,device_revoke,mark_void,override_delete,",
            "override_set,schedule_set,settings_update,staff_create,staff_remove,staff_update",
        ]
        .concat()
    );
    // Every RPC that takes a secret checks it (admin RPCs: with the session).
    assert_eq!(
        dev.text(&public_rpcs(
            "p.proargnames[1] = 'p_secret'
             and p.prosrc not like '%app.auth_device(p_secret)%'
             and p.prosrc not like '%app.auth_admin(p_secret, p_session)%'"
        ))
        .await,
        ""
    );
    assert_eq!(
        dev.text(&public_rpcs(
            "p.proargnames[2] = 'p_session' and p.proname <> 'admin_logout'
             and p.prosrc not like '%app.auth_admin(p_secret, p_session)%'"
        ))
        .await,
        ""
    );
}

/// Without a valid device secret, nothing can be read or written.
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn unpaired_calls_are_denied() {
    let dev = dev().await;
    let api = &dev.api;

    // Before initialisation, pairing isn't possible.
    assert_eq!(
        code(
            api.pair_device(&throwaway_passphrase(), "X", Platform::Android)
                .await
        ),
        ErrorCode::NotInitialized
    );
    // admin_initialize validates its input.
    let pp = throwaway_passphrase();
    assert_eq!(
        code(
            api.admin_initialize("too short", &quit(), "PC", Platform::Windows)
                .await
        ),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        code(
            api.admin_initialize(&pp, &QuitCode::new("12a4".into()), "PC", Platform::Windows)
                .await
        ),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        code(
            api.admin_initialize(&pp, &quit(), " ", Platform::Windows)
                .await
        ),
        ErrorCode::InvalidInput
    );

    let (device, passphrase) = dev.init().await;
    // Only once.
    assert_eq!(
        code(
            api.admin_initialize(&throwaway_passphrase(), &quit(), "PC2", Platform::Windows)
                .await
        ),
        ErrorCode::AlreadyInitialized
    );
    assert_eq!(dev.i64("select count(*) from app.devices").await, 1);

    let session = dev.login(&device, &passphrase).await;
    let staff = api
        .staff_create(
            &device.device_secret,
            &session,
            "Νίκος",
            "Test",
            &week(&[(Weekday::Monday, t(9, 0), t(17, 0))]),
        )
        .await
        .unwrap();

    for bad in [random_secret(), DeviceSecret::new(String::new())] {
        assert_eq!(code(api.get_version(&bad).await), ErrorCode::BadSecret);
        assert_eq!(code(api.get_snapshot(&bad).await), ErrorCode::BadSecret);
        assert_eq!(code(api.get_plan(&bad).await), ErrorCode::BadSecret);
        assert_eq!(
            code(
                api.check_alarm(&bad, &[("0".repeat(32).as_str(), Timestamp::now())])
                    .await
            ),
            ErrorCode::BadSecret
        );
        assert_eq!(
            code(
                api.mark(
                    &bad,
                    &MarkRequest {
                        client_id: Uuid::new_v4(),
                        staff_id: staff,
                        source_block_id: Uuid::new_v4(),
                        business_date: dev.business_today().await,
                        kind: MarkKind::In,
                    }
                )
                .await
            ),
            ErrorCode::BadSecret
        );
        assert_eq!(
            code(api.upload_plan(&bad, 0, Timestamp::now(), &[]).await),
            ErrorCode::BadSecret
        );
        assert_eq!(
            code(api.admin_login(&bad, &passphrase).await),
            ErrorCode::BadSecret
        );
        // Even with a real session token, the admin RPCs need the device.
        assert_eq!(
            code(api.staff_remove(&bad, &session, staff).await),
            ErrorCode::BadSecret
        );
        assert_eq!(
            code(
                api.staff_create(
                    &bad,
                    &session,
                    "A",
                    "B",
                    &week(&[(Weekday::Monday, t(9, 0), t(17, 0))])
                )
                .await
            ),
            ErrorCode::BadSecret
        );
    }
    // Nothing was written by the denied calls.
    assert_eq!(dev.i64("select count(*) from app.marks").await, 0);
    assert_eq!(
        dev.i64("select count(*) from app.staff where removed_at is null")
            .await,
        1
    );
    assert_eq!(dev.i64("select count(*) from app.alarm_plan").await, 0);
}

/// Pairing a second device, and what the snapshot contains.
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn pairing_and_snapshot() {
    let dev = dev().await;
    let api = &dev.api;
    let (pc, passphrase) = dev.init().await;
    let phone = api
        .pair_device(
            &format!("  {}  ", passphrase.to_uppercase()),
            "Phone",
            Platform::Android,
        )
        .await
        .expect("normalised passphrase pairs");
    assert_ne!(pc.device_secret, phone.device_secret);
    // Only hashes are stored.
    assert_eq!(
        dev.i64("select count(*) from app.devices where octet_length(secret_hash) = 32")
            .await,
        2
    );

    let snap = api.get_snapshot(&phone.device_secret).await.unwrap();
    assert_eq!(snap.this_device_id, phone.device_id);
    assert_eq!(snap.devices.len(), 2);
    assert_eq!(snap.settings.quit_code.as_ref().unwrap().expose(), QUIT);
    assert_eq!(snap.settings.rollover, t(5, 0));
    assert!(snap.staff.is_empty());
    let skew = snap.server_now.duration_since(Timestamp::now()).abs();
    assert!(
        skew < SignedDuration::from_secs(60),
        "server clock skew {skew:?}"
    );

    let v = api.get_version(&pc.device_secret).await.unwrap();
    assert_eq!(v.data_version, snap.data_version);
    assert_eq!(v.config_version, snap.config_version);
}

/// The lockout: 5 wrong attempts lock for 1 minute, each further failure
/// doubles it up to 60 minutes, the counter is stored (so it survives a
/// restart and is not rolled back), and a success resets it.
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn passphrase_lockout() {
    let dev = dev().await;
    let api = &dev.api;
    let (pc, passphrase) = dev.init().await;
    let wrong = throwaway_passphrase();

    for n in 1..=4 {
        let r = api.pair_device(&wrong, "Phone", Platform::Android).await;
        assert_eq!(retry_after(r), None, "attempt {n}");
        assert_eq!(
            dev.i64("select failed_count::bigint from app.auth_state")
                .await,
            n,
            "the counter must persist after a failed attempt"
        );
    }
    // 5th failure (via admin_login: the counter is shared) → 1 minute.
    let r = api.admin_login(&pc.device_secret, &wrong).await;
    let secs = retry_after(r).expect("5th failure locks");
    assert!((58..=60).contains(&secs), "{secs}");

    // While locked, even the right passphrase is refused, and attempts don't count.
    let r = api
        .pair_device(&passphrase, "Phone", Platform::Android)
        .await;
    assert!(matches!(&r, Err(ApiError::Rejected(x)) if x.error == ErrorCode::Locked));
    assert!(retry_after(r).unwrap() > 0);
    assert_eq!(
        code(api.admin_login(&pc.device_secret, &wrong).await),
        ErrorCode::Locked
    );
    assert_eq!(
        dev.i64("select failed_count::bigint from app.auth_state")
            .await,
        5
    );
    assert_eq!(dev.i64("select count(*) from app.devices").await, 1);

    // Lock over → the 6th failure locks for 2 minutes.
    dev.exec("update app.auth_state set locked_until = now() - interval '1 second' where id")
        .await;
    let secs = retry_after(api.pair_device(&wrong, "P", Platform::Android).await).unwrap();
    assert!((118..=120).contains(&secs), "{secs}");
    dev.exec("update app.auth_state set locked_until = now() - interval '1 second' where id")
        .await;
    let secs = retry_after(api.pair_device(&wrong, "P", Platform::Android).await).unwrap();
    assert!((238..=240).contains(&secs), "{secs}");

    // Capped at 60 minutes.
    dev.exec(
        "update app.auth_state set failed_count = 40, locked_until = now() - interval '1 second' where id",
    )
    .await;
    let secs = retry_after(api.pair_device(&wrong, "P", Platform::Android).await).unwrap();
    assert!((3598..=3600).contains(&secs), "{secs}");

    // A correct entry after the lock resets everything.
    dev.exec("update app.auth_state set locked_until = now() - interval '1 second' where id")
        .await;
    api.pair_device(&passphrase, "Phone", Platform::Android)
        .await
        .unwrap();
    assert_eq!(
        dev.i64("select failed_count::bigint from app.auth_state")
            .await,
        0
    );
    assert!(
        dev.boolean("select locked_until is null from app.auth_state")
            .await
    );
}

/// Marks are idempotent on the client id, and a second device marking the
/// same occurrence gets the first mark back.
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn marks_are_idempotent() {
    let dev = dev().await;
    let api = &dev.api;
    let (pc, passphrase) = dev.init().await;
    let phone = api
        .pair_device(&passphrase, "Phone", Platform::Android)
        .await
        .unwrap();
    let session = dev.login(&pc, &passphrase).await;
    let staff = api
        .staff_create(
            &pc.device_secret,
            &session,
            "Μαρία",
            "Δοκιμή",
            &week(&[(Weekday::Monday, t(12, 0), t(16, 0))]),
        )
        .await
        .unwrap();
    let block = api.get_snapshot(&pc.device_secret).await.unwrap().blocks[0].id;
    let today = dev.business_today().await;

    let request = MarkRequest {
        client_id: Uuid::new_v4(),
        staff_id: staff,
        source_block_id: block,
        business_date: today,
        kind: MarkKind::In,
    };
    let before = api.get_version(&pc.device_secret).await.unwrap();
    let first = api.mark(&pc.device_secret, &request).await.unwrap();
    assert_eq!(first.mark_id, request.client_id);
    let again = api.mark(&pc.device_secret, &request).await.unwrap();
    assert_eq!(again, first);
    let other_device = api
        .mark(
            &phone.device_secret,
            &MarkRequest {
                client_id: Uuid::new_v4(),
                ..request.clone()
            },
        )
        .await
        .unwrap();
    assert_eq!(other_device.mark_id, first.mark_id);
    assert_eq!(dev.i64("select count(*) from app.marks").await, 1);
    let after = api.get_version(&pc.device_secret).await.unwrap();
    assert!(after.data_version > before.data_version);
    assert_eq!(after.config_version, before.config_version);

    // Check-out is a separate mark.
    let out = api
        .mark(
            &phone.device_secret,
            &MarkRequest {
                client_id: Uuid::new_v4(),
                kind: MarkKind::Out,
                ..request.clone()
            },
        )
        .await
        .unwrap();
    assert_ne!(out.mark_id, first.mark_id);
    let snap = api.get_snapshot(&phone.device_secret).await.unwrap();
    assert_eq!(snap.marks.len(), 2);
    assert_eq!(snap.marks[0].device_id, Some(pc.device_id));

    // Out-of-range dates and unknown staff are refused.
    for bad in [
        MarkRequest {
            client_id: Uuid::new_v4(),
            business_date: today.checked_sub(jiff::Span::new().days(3)).unwrap(),
            ..request.clone()
        },
        MarkRequest {
            client_id: Uuid::new_v4(),
            staff_id: Uuid::new_v4(),
            ..request.clone()
        },
    ] {
        assert_eq!(
            code(api.mark(&pc.device_secret, &bad).await),
            ErrorCode::InvalidInput
        );
    }
    // Two days back is still accepted (offline marks arriving late).
    let late = MarkRequest {
        client_id: Uuid::new_v4(),
        business_date: today.checked_sub(jiff::Span::new().days(2)).unwrap(),
        ..request.clone()
    };
    api.mark(&pc.device_secret, &late).await.unwrap();
}

/// Removing a mark needs an admin session of the calling device.
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn mark_void_is_admin_only() {
    let dev = dev().await;
    let api = &dev.api;
    let (pc, passphrase) = dev.init().await;
    let phone = api
        .pair_device(&passphrase, "Phone", Platform::Android)
        .await
        .unwrap();
    let session = dev.login(&pc, &passphrase).await;
    let staff = api
        .staff_create(
            &pc.device_secret,
            &session,
            "Κώστας",
            "Δοκιμή",
            &week(&[(Weekday::Tuesday, t(9, 0), t(17, 0))]),
        )
        .await
        .unwrap();
    let block = api.get_snapshot(&pc.device_secret).await.unwrap().blocks[0].id;
    let request = MarkRequest {
        client_id: Uuid::new_v4(),
        staff_id: staff,
        source_block_id: block,
        business_date: dev.business_today().await,
        kind: MarkKind::In,
    };
    let mark = api.mark(&phone.device_secret, &request).await.unwrap();

    // No session, a made-up session, another device's session: all refused.
    for (secret, token) in [
        (&phone.device_secret, SessionToken::new(String::new())),
        (&phone.device_secret, SessionToken::new("made-up".into())),
        (&phone.device_secret, session.clone()),
    ] {
        assert_eq!(
            code(api.mark_void(secret, &token, mark.mark_id).await),
            ErrorCode::NoSession
        );
    }
    assert_eq!(
        dev.i64("select count(*) from app.marks where voided_at is null")
            .await,
        1
    );

    assert!(
        api.mark_void(&pc.device_secret, &session, mark.mark_id)
            .await
            .unwrap()
    );
    assert!(
        !api.mark_void(&pc.device_secret, &session, mark.mark_id)
            .await
            .unwrap()
    );
    assert!(
        api.get_snapshot(&phone.device_secret)
            .await
            .unwrap()
            .marks
            .is_empty()
    );
    // After a correction the person can be marked again.
    let again = api
        .mark(
            &phone.device_secret,
            &MarkRequest {
                client_id: Uuid::new_v4(),
                ..request
            },
        )
        .await
        .unwrap();
    assert_ne!(again.mark_id, mark.mark_id);
}

/// Every admin RPC refuses a missing or expired session; sessions slide,
/// belong to one device, and end on logout, revoke or passphrase change.
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn admin_sessions() {
    let dev = dev().await;
    let api = &dev.api;
    let (pc, passphrase) = dev.init().await;
    let phone = api
        .pair_device(&passphrase, "Phone", Platform::Android)
        .await
        .unwrap();
    let secret = &pc.device_secret;
    let none = SessionToken::new("not-a-session".into());
    let blocks = week(&[(Weekday::Monday, t(9, 0), t(17, 0))]);
    let settings = SettingsInput {
        checkin_offset_min: -5,
        checkout_offset_min: 10,
        rollover: t(4, 0),
        autostart: false,
        quit_code: QuitCode::new("9876".into()),
    };
    let id = Uuid::new_v4();
    let today = dev.business_today().await;

    macro_rules! all_admin_calls_refused {
        ($secret:expr, $token:expr) => {{
            let (s, k) = ($secret, $token);
            assert_eq!(
                code(api.staff_create(s, k, "A", "B", &blocks).await),
                ErrorCode::NoSession
            );
            assert_eq!(
                code(api.staff_update(s, k, id, "A", "B").await),
                ErrorCode::NoSession
            );
            assert_eq!(code(api.staff_remove(s, k, id).await), ErrorCode::NoSession);
            assert_eq!(
                code(api.schedule_set(s, k, id, &blocks).await),
                ErrorCode::NoSession
            );
            assert_eq!(
                code(
                    api.override_set(s, k, id, today, OverrideKind::Off, &[])
                        .await
                ),
                ErrorCode::NoSession
            );
            assert_eq!(
                code(api.override_delete(s, k, id).await),
                ErrorCode::NoSession
            );
            assert_eq!(
                code(api.settings_update(s, k, &settings).await),
                ErrorCode::NoSession
            );
            assert_eq!(code(api.mark_void(s, k, id).await), ErrorCode::NoSession);
            assert_eq!(
                code(api.device_revoke(s, k, id).await),
                ErrorCode::NoSession
            );
            assert_eq!(
                code(
                    api.change_passphrase(s, k, &passphrase, &throwaway_passphrase())
                        .await
                ),
                ErrorCode::NoSession
            );
        }};
    }

    all_admin_calls_refused!(secret, &none);
    assert_eq!(
        dev.i64("select failed_count::bigint from app.auth_state")
            .await,
        0
    );

    let session = dev.login(&pc, &passphrase).await;
    // Another device can't use this device's session.
    all_admin_calls_refused!(&phone.device_secret, &session);

    // Sliding expiry: each admin call pushes it to 10 minutes from now.
    dev.exec("update app.admin_sessions set expires_at = now() + interval '1 minute'")
        .await;
    api.settings_update(secret, &session, &settings)
        .await
        .unwrap();
    assert!(
        dev.boolean("select expires_at > now() + interval '9 minutes' from app.admin_sessions")
            .await
    );
    let snap = api.get_snapshot(secret).await.unwrap();
    assert_eq!(snap.settings.checkin_offset_min, -5);
    assert_eq!(snap.settings.rollover, t(4, 0));
    assert!(!snap.settings.autostart);
    assert_eq!(snap.settings.quit_code.unwrap().expose(), "9876");

    // Expired → refused.
    dev.exec("update app.admin_sessions set expires_at = now() - interval '1 second'")
        .await;
    all_admin_calls_refused!(secret, &session);

    // Logout ends it.
    let session = dev.login(&pc, &passphrase).await;
    api.staff_create(secret, &session, "A", "B", &blocks)
        .await
        .unwrap();
    api.admin_logout(secret, &session).await.unwrap();
    all_admin_calls_refused!(secret, &session);

    // Revoking a device ends its sessions and its access.
    let phone_session = dev.login(&phone, &passphrase).await;
    let session = dev.login(&pc, &passphrase).await;
    api.device_revoke(secret, &session, phone.device_id)
        .await
        .unwrap();
    assert_eq!(
        code(api.get_version(&phone.device_secret).await),
        ErrorCode::Revoked
    );
    assert_eq!(
        code(
            api.staff_remove(&phone.device_secret, &phone_session, id)
                .await
        ),
        ErrorCode::Revoked
    );
    assert_eq!(
        dev.i64(&format!(
            "select count(*) from app.admin_sessions where device_id = '{}'",
            phone.device_id
        ))
        .await,
        0
    );
    assert_eq!(api.get_snapshot(secret).await.unwrap().devices.len(), 1);

    // Changing the passphrase: wrong current one counts as a failed attempt,
    // an invalid new one is refused, and success signs everyone out.
    let new_passphrase = throwaway_passphrase();
    assert_eq!(
        code(
            api.change_passphrase(secret, &session, &throwaway_passphrase(), &new_passphrase)
                .await
        ),
        ErrorCode::BadPassphrase
    );
    assert_eq!(
        dev.i64("select failed_count::bigint from app.auth_state")
            .await,
        1
    );
    assert_eq!(
        code(
            api.change_passphrase(secret, &session, &passphrase, "one two three")
                .await
        ),
        ErrorCode::InvalidInput
    );
    api.change_passphrase(secret, &session, &passphrase, &new_passphrase)
        .await
        .unwrap();
    assert_eq!(dev.i64("select count(*) from app.admin_sessions").await, 0);
    all_admin_calls_refused!(secret, &session);
    assert_eq!(
        code(api.admin_login(secret, &passphrase).await),
        ErrorCode::BadPassphrase
    );
    dev.login(&pc, &new_passphrase).await;
    // Paired devices stay paired.
    api.get_version(secret).await.unwrap();
}

/// Staff, weekly schedules, overrides and settings: what the server accepts
/// and that edited blocks keep their ids.
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn admin_data_changes() {
    let dev = dev().await;
    let api = &dev.api;
    let (pc, passphrase) = dev.init().await;
    let secret = &pc.device_secret;
    let session = dev.login(&pc, &passphrase).await;
    let v0 = api.get_version(secret).await.unwrap();

    let staff = api
        .staff_create(
            secret,
            &session,
            "  Γιώργος ",
            "Ο’Νιλ",
            &week(&[
                (Weekday::Monday, t(9, 0), t(17, 0)),
                (Weekday::Wednesday, t(9, 0), t(17, 0)),
            ]),
        )
        .await
        .unwrap();
    let v1 = api.get_version(secret).await.unwrap();
    assert!(v1.config_version > v0.config_version);
    let snap = api.get_snapshot(secret).await.unwrap();
    assert_eq!(snap.staff[0].first_name, "Γιώργος");
    let id_of = |snap: &clockin_sync::ServerSnapshot, day: Weekday| {
        snap.blocks.iter().find(|b| b.weekday == day).map(|b| b.id)
    };
    let monday = id_of(&snap, Weekday::Monday).unwrap();
    let wednesday = id_of(&snap, Weekday::Wednesday).unwrap();

    // Marks made before the edit (the server doesn't check the weekday).
    let today = dev.business_today().await;
    for block in [monday, wednesday] {
        api.mark(
            secret,
            &MarkRequest {
                client_id: Uuid::new_v4(),
                staff_id: staff,
                source_block_id: block,
                business_date: today,
                kind: MarkKind::In,
            },
        )
        .await
        .unwrap();
    }

    // Edit Monday by id, keep Wednesday unchanged without an id, add Friday.
    let new_week = vec![
        WeekBlockInput {
            id: Some(monday),
            weekday: Weekday::Monday,
            start: t(10, 0),
            end: t(17, 0),
        },
        WeekBlockInput {
            id: None,
            weekday: Weekday::Wednesday,
            start: t(9, 0),
            end: t(17, 0),
        },
        WeekBlockInput {
            id: None,
            weekday: Weekday::Friday,
            start: t(19, 0),
            end: t(2, 0),
        },
    ];
    api.schedule_set(secret, &session, staff, &new_week)
        .await
        .unwrap();
    let snap = api.get_snapshot(secret).await.unwrap();
    assert_eq!(snap.blocks.len(), 3);
    assert_eq!(id_of(&snap, Weekday::Monday), Some(monday));
    assert_eq!(id_of(&snap, Weekday::Wednesday), Some(wednesday));
    let mon = snap.blocks.iter().find(|b| b.id == monday).unwrap();
    assert_eq!((mon.start, mon.end), (t(10, 0), t(17, 0)));
    // Both marks still point at a block that exists: edited (Monday) and
    // unchanged (Wednesday) blocks kept their ids.
    assert_eq!(snap.marks.len(), 2);
    for mark in &snap.marks {
        assert!(
            snap.blocks.iter().any(|b| b.id == mark.source_block_id),
            "a mark lost its block"
        );
    }
    let core = snap.core_snapshot(&[]);
    for block in [monday, wednesday] {
        assert!(core.marks.iter().any(|m| m.source_block_id == block));
    }

    // Dropping a block deletes it.
    api.schedule_set(secret, &session, staff, &new_week[..1])
        .await
        .unwrap();
    assert_eq!(api.get_snapshot(secret).await.unwrap().blocks.len(), 1);

    // Invalid schedules are refused and change nothing.
    let invalid_weeks: Vec<Vec<WeekBlockInput>> =
        vec![vec![], week(&[(Weekday::Monday, t(9, 0), t(9, 0))])];
    for bad in &invalid_weeks {
        assert_eq!(
            code(api.schedule_set(secret, &session, staff, bad).await),
            ErrorCode::InvalidInput
        );
    }
    assert_eq!(api.get_snapshot(secret).await.unwrap().blocks.len(), 1);
    assert_eq!(
        code(api.staff_create(secret, &session, "", "X", &new_week).await),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        code(
            api.staff_create(secret, &session, &"α".repeat(41), "X", &new_week)
                .await
        ),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        code(api.staff_create(secret, &session, "A", "B", &[]).await),
        ErrorCode::InvalidInput
    );
    assert_eq!(dev.i64("select count(*) from app.staff").await, 1);

    api.staff_update(secret, &session, staff, "Γιώργος", "Νέος")
        .await
        .unwrap();
    assert_eq!(
        api.get_snapshot(secret).await.unwrap().staff[0].last_name,
        "Νέος"
    );

    // Overrides: today or later only; off has no blocks, replace has some.
    let yesterday = today.yesterday().unwrap();
    let tomorrow = today.tomorrow().unwrap();
    let replacement = [
        OverrideBlockInput {
            id: None,
            start: t(10, 0),
            end: t(14, 0),
        },
        OverrideBlockInput {
            id: None,
            start: t(19, 0),
            end: t(0, 0),
        },
    ];
    assert_eq!(
        code(
            api.override_set(secret, &session, staff, yesterday, OverrideKind::Off, &[])
                .await
        ),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        code(
            api.override_set(
                secret,
                &session,
                staff,
                today,
                OverrideKind::Off,
                &replacement
            )
            .await
        ),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        code(
            api.override_set(secret, &session, staff, today, OverrideKind::Replace, &[])
                .await
        ),
        ErrorCode::InvalidInput
    );
    let o = api
        .override_set(
            secret,
            &session,
            staff,
            tomorrow,
            OverrideKind::Replace,
            &replacement,
        )
        .await
        .unwrap();
    let snap = api.get_snapshot(secret).await.unwrap();
    assert_eq!(snap.overrides.len(), 1);
    assert_eq!(snap.overrides[0].business_date, tomorrow);
    assert_eq!(snap.overrides[0].blocks.len(), 2);
    let kept = snap.overrides[0].blocks[0].id;
    // Same person and date again: the same override, now a day off.
    let o2 = api
        .override_set(secret, &session, staff, tomorrow, OverrideKind::Off, &[])
        .await
        .unwrap();
    assert_eq!(o2, o);
    let snap = api.get_snapshot(secret).await.unwrap();
    assert_eq!(snap.overrides[0].kind, OverrideKind::Off);
    assert!(snap.overrides[0].blocks.is_empty());
    // Back to replace-hours with an unchanged block → a new id is fine,
    // and editing by id keeps it.
    api.override_set(
        secret,
        &session,
        staff,
        tomorrow,
        OverrideKind::Replace,
        &replacement[..1],
    )
    .await
    .unwrap();
    let kept2 = api.get_snapshot(secret).await.unwrap().overrides[0].blocks[0].id;
    assert_ne!(kept2, kept);
    api.override_set(
        secret,
        &session,
        staff,
        tomorrow,
        OverrideKind::Replace,
        &[OverrideBlockInput {
            id: Some(kept2),
            start: t(11, 0),
            end: t(14, 0),
        }],
    )
    .await
    .unwrap();
    let snap = api.get_snapshot(secret).await.unwrap();
    assert_eq!(snap.overrides[0].blocks[0].id, kept2);
    assert_eq!(snap.overrides[0].blocks[0].start, t(11, 0));

    assert!(api.override_delete(secret, &session, o).await.unwrap());
    assert!(!api.override_delete(secret, &session, o).await.unwrap());

    // Settings ranges.
    let ok = SettingsInput {
        checkin_offset_min: 0,
        checkout_offset_min: 0,
        rollover: t(5, 0),
        autostart: true,
        quit_code: quit(),
    };
    for bad in [
        SettingsInput {
            checkin_offset_min: -61,
            ..ok.clone()
        },
        SettingsInput {
            checkout_offset_min: 31,
            ..ok.clone()
        },
        SettingsInput {
            rollover: t(8, 1),
            ..ok.clone()
        },
        SettingsInput {
            quit_code: QuitCode::new("12345".into()),
            ..ok.clone()
        },
    ] {
        assert_eq!(
            code(api.settings_update(secret, &session, &bad).await),
            ErrorCode::InvalidInput
        );
    }
    api.settings_update(secret, &session, &ok).await.unwrap();

    // Removing someone keeps them in the snapshot for a while (an
    // in-progress block stays), but they can no longer be edited.
    api.staff_remove(secret, &session, staff).await.unwrap();
    let snap = api.get_snapshot(secret).await.unwrap();
    assert!(snap.staff[0].removed_at.is_some());
    assert_eq!(snap.blocks.len(), 1);
    assert_eq!(
        code(api.schedule_set(secret, &session, staff, &new_week).await),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        code(api.staff_remove(secret, &session, staff).await),
        ErrorCode::InvalidInput
    );
}

/// `upload_plan` refuses a plan computed from an outdated configuration;
/// `check_alarm` answers per (item id, time).
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn alarm_plan_upload_and_check() {
    let dev = dev().await;
    let api = &dev.api;
    let (pc, passphrase) = dev.init().await;
    let secret = &pc.device_secret;
    let session = dev.login(&pc, &passphrase).await;
    // Every day, so the 14-day plan has items whatever today is.
    let every_day: Vec<_> = [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ]
    .iter()
    .map(|&d| (d, t(12, 0), t(16, 0)))
    .collect();
    api.staff_create(secret, &session, "Ελένη", "Δοκιμή", &week(&every_day))
        .await
        .unwrap();

    let snap = api.get_snapshot(secret).await.unwrap();
    let now = Timestamp::now();
    let (from, until) = plan_window(now);
    let items = alarm_plan(&snap.core_snapshot(&[]), from, until);
    assert!(items.len() >= 26, "{}", items.len());

    assert_eq!(
        code(
            api.upload_plan(secret, snap.config_version - 1, until, &items)
                .await
        ),
        ErrorCode::VersionConflict
    );
    assert_eq!(
        api.upload_plan(secret, snap.config_version, until, &items)
            .await
            .unwrap() as usize,
        items.len()
    );
    let v = api.get_version(secret).await.unwrap();
    assert_eq!(v.plan_config_version, snap.config_version);
    // Postgres keeps microseconds; `until` has nanoseconds.
    let same_instant = |a: Option<Timestamp>| {
        a.is_some_and(|a| a.duration_since(until).abs() < SignedDuration::from_millis(1))
    };
    assert!(same_instant(v.plan_horizon_end));

    let plan = api.get_plan(secret).await.unwrap();
    assert_eq!(plan.items, items);
    assert!(same_instant(plan.horizon_end));

    // A later item: due at its own time only.
    let item = items.iter().find(|i| i.kind == MarkKind::In).unwrap();
    let check = |at: Timestamp| {
        let id = item.item_id.clone();
        async move {
            api.check_alarm(secret, &[(id.as_str(), at), ("0".repeat(32).as_str(), at)])
                .await
                .unwrap()
        }
    };
    let c = check(item.fires_at).await;
    assert_eq!(c.config_version, v.config_version);
    assert_eq!(c.items[0].item_id, item.item_id);
    assert!(c.items[0].due);
    assert!(!c.items[1].due, "unknown ids are not due");
    let moved = item.fires_at + SignedDuration::from_mins(30);
    assert!(
        !check(moved).await.items[0].due,
        "a moved alarm is not due at the new time"
    );

    // A matching mark suppresses it; voiding the mark brings it back.
    let mark = api
        .mark(
            secret,
            &MarkRequest {
                client_id: Uuid::new_v4(),
                staff_id: item.staff_id,
                source_block_id: item.source_block_id,
                business_date: item.business_date,
                kind: MarkKind::In,
            },
        )
        .await;
    // The item may lie beyond tomorrow, where marks aren't accepted; then
    // insert the mark directly.
    let mark_id = match mark {
        Ok(r) => r.mark_id,
        Err(_) => {
            let id = Uuid::new_v4();
            dev.exec(&format!(
                "insert into app.marks (id, staff_id, source_block_id, business_date, kind)
                 values ('{id}', '{}', '{}', '{}', 'in')",
                item.staff_id, item.source_block_id, item.business_date
            ))
            .await;
            id
        }
    };
    assert!(!check(item.fires_at).await.items[0].due);
    api.mark_void(secret, &session, mark_id).await.unwrap();
    assert!(check(item.fires_at).await.items[0].due);

    // Any configuration change makes the old base stale.
    api.staff_create(
        secret,
        &session,
        "Άλλος",
        "Δοκιμή",
        &week(&[(Weekday::Monday, t(9, 0), t(10, 0))]),
    )
    .await
    .unwrap();
    let r = api
        .upload_plan(secret, snap.config_version, until, &items)
        .await;
    let Err(ApiError::Rejected(rejection)) = r else {
        panic!("expected version_conflict, got {r:?}")
    };
    assert_eq!(rejection.error, ErrorCode::VersionConflict);
    assert!(rejection.config_version.unwrap() > snap.config_version);
    // The old plan is untouched.
    assert_eq!(api.get_plan(secret).await.unwrap().items.len(), items.len());

    // Malformed items are refused without touching the plan.
    let v = api.get_version(secret).await.unwrap();
    let mut bad = items.clone();
    bad[0].item_id = "not-hex".into();
    assert_eq!(
        code(api.upload_plan(secret, v.config_version, until, &bad).await),
        ErrorCode::InvalidInput
    );
    assert_eq!(api.get_plan(secret).await.unwrap().items.len(), items.len());
}

/// The daily purge (SPEC §9) and its pg_cron job.
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn purge_removes_only_expired_data() {
    let dev = dev().await;
    let api = &dev.api;
    let (pc, passphrase) = dev.init().await;
    let secret = &pc.device_secret;
    let session = dev.login(&pc, &passphrase).await;
    let blocks = week(&[(Weekday::Monday, t(9, 0), t(17, 0))]);
    let active = api
        .staff_create(secret, &session, "Ενεργός", "Δοκιμή", &blocks)
        .await
        .unwrap();
    let gone = api
        .staff_create(secret, &session, "Παλιός", "Δοκιμή", &blocks)
        .await
        .unwrap();
    let gone_with_marks = api
        .staff_create(secret, &session, "Παλιός", "Σημάδια", &blocks)
        .await
        .unwrap();
    let just_removed = api
        .staff_create(secret, &session, "Μόλις", "Έφυγε", &blocks)
        .await
        .unwrap();
    for id in [gone, gone_with_marks, just_removed] {
        api.staff_remove(secret, &session, id).await.unwrap();
    }
    dev.exec(&format!(
        "update app.staff set removed_at = now() - interval '4 days' where id in ('{gone}', '{gone_with_marks}');
         insert into app.marks (id, staff_id, source_block_id, business_date, kind) values
           (gen_random_uuid(), '{active}', gen_random_uuid(), app.business_today() - 30, 'in'),
           (gen_random_uuid(), '{active}', gen_random_uuid(), app.business_today() - 31, 'out'),
           (gen_random_uuid(), '{active}', gen_random_uuid(), app.business_today() - 29, 'in'),
           (gen_random_uuid(), '{gone_with_marks}', gen_random_uuid(), app.business_today() - 5, 'in');
         insert into app.overrides (staff_id, business_date, kind) values
           ('{active}', app.business_today() - 30, 'off'),
           ('{active}', app.business_today() - 29, 'off'),
           ('{active}', app.business_today() + 1, 'off');
         insert into app.admin_sessions (token_hash, device_id, expires_at) values
           (sha256('expired'::bytea), '{}', now() - interval '1 minute');",
        pc.device_id
    ))
    .await;

    let result: serde_json::Value = dev
        .db
        .query_one("select app.purge()", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(result["marks"], 2);
    assert_eq!(result["overrides"], 1);
    assert_eq!(result["staff"], 1);
    assert_eq!(result["sessions"], 1);

    assert_eq!(dev.i64("select count(*) from app.marks").await, 2);
    assert_eq!(dev.i64("select count(*) from app.overrides").await, 2);
    assert_eq!(
        dev.i64(&format!(
            "select count(*) from app.staff where id = '{gone}'"
        ))
        .await,
        0
    );
    assert_eq!(dev.i64("select count(*) from app.staff").await, 3);
    // The live session survived.
    api.staff_update(secret, &session, active, "Ενεργός", "Ακόμα")
        .await
        .unwrap();

    assert_eq!(
        dev.i64(
            "select count(*) from cron.job
              where jobname = 'clockin-purge' and command = 'select app.purge()' and active"
        )
        .await,
        1
    );
}

/// A removed person is purged only once their removal is older than the
/// snapshot window and no mark references them, so nobody the devices can
/// still see (e.g. someone removed mid-shift today) is ever deleted.
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn purge_keeps_removed_staff_inside_the_snapshot_window() {
    let dev = dev().await;
    let api = &dev.api;
    let (pc, passphrase) = dev.init().await;
    let secret = &pc.device_secret;
    let session = dev.login(&pc, &passphrase).await;
    let blocks = week(&[(Weekday::Monday, t(19, 0), t(2, 0))]);

    // (first name, removed this long ago, has a mark in the window)
    let cases = [
        ("Σήμερα", "1 hour", false),
        ("Χθες", "1 day", false),
        ("Σχεδόν", "2 days 23 hours", false),
        ("Παλιός", "3 days 1 minute", false),
        ("Σημάδι", "3 days 1 minute", true),
    ];
    let mut ids = Vec::new();
    for (name, ago, with_mark) in cases {
        let id = api
            .staff_create(secret, &session, name, "Δοκιμή", &blocks)
            .await
            .unwrap();
        api.staff_remove(secret, &session, id).await.unwrap();
        dev.exec(&format!(
            "update app.staff set removed_at = now() - interval '{ago}' where id = '{id}'"
        ))
        .await;
        if with_mark {
            dev.exec(&format!(
                "insert into app.marks (id, staff_id, source_block_id, business_date, kind)
                 values (gen_random_uuid(), '{id}', gen_random_uuid(), app.business_today() - 1, 'in')"
            ))
            .await;
        }
        ids.push(id);
    }

    let in_snapshot =
        |snap: &clockin_sync::ServerSnapshot, id: Uuid| snap.staff.iter().any(|s| s.id == id);
    let before = api.get_snapshot(secret).await.unwrap();
    for (i, (name, _, _)) in cases.iter().enumerate() {
        let expected = *name != "Παλιός";
        assert_eq!(in_snapshot(&before, ids[i]), expected, "{name} in snapshot");
    }

    let result: serde_json::Value = dev
        .db
        .query_one("select app.purge()", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(result["staff"], 1);

    for (i, (name, _, _)) in cases.iter().enumerate() {
        let exists = dev
            .i64(&format!(
                "select count(*) from app.staff where id = '{}'",
                ids[i]
            ))
            .await
            == 1;
        assert_eq!(exists, *name != "Παλιός", "{name} after purge");
        // Invariant: the purge never deletes anyone the snapshot still shows.
        if in_snapshot(&before, ids[i]) {
            assert!(exists, "{name} was in the snapshot but got purged");
        }
    }
    // Their blocks went with them; everyone else's stayed.
    assert_eq!(dev.i64("select count(*) from app.schedule_blocks").await, 4);
}

/// The real sync engine against dev: snapshot cached, plan uploaded, an
/// offline-queued mark delivered.
#[tokio::test]
#[ignore = "uses the dev Supabase project"]
async fn sync_engine_end_to_end() {
    let dev = dev().await;
    let api = &dev.api;
    let (pc, passphrase) = dev.init().await;
    let session = dev.login(&pc, &passphrase).await;
    let staff = api
        .staff_create(
            &pc.device_secret,
            &session,
            "Σοφία",
            "Δοκιμή",
            &week(&[
                (Weekday::Monday, t(12, 0), t(16, 0)),
                (Weekday::Monday, t(19, 0), t(0, 0)),
                (Weekday::Friday, t(18, 0), t(2, 0)),
            ]),
        )
        .await
        .unwrap();

    let store = Store::open_in_memory(&StoreKey::generate().unwrap()).unwrap();
    let mut engine = SyncEngine::new(api.clone(), store, pc.device_secret.clone()).unwrap();
    let report = engine.tick(Timestamp::now()).await.unwrap();
    assert!(report.data_changed);
    assert_eq!(report.next_tick_in, SignedDuration::from_secs(5));
    assert!(engine.status().online && engine.status().paired);
    assert!(!engine.status().clock_skew);
    assert_eq!(engine.snapshot().unwrap().blocks.len(), 3);
    assert!(!engine.horizon_short(Timestamp::now()));

    let v = api.get_version(&pc.device_secret).await.unwrap();
    assert_eq!(v.plan_config_version, v.config_version);
    assert!(
        !api.get_plan(&pc.device_secret)
            .await
            .unwrap()
            .items
            .is_empty()
    );

    // Nothing changed → nothing refetched.
    let report = engine.tick(Timestamp::now()).await.unwrap();
    assert!(!report.data_changed);

    let block = engine.snapshot().unwrap().blocks[0].id;
    let today = dev.business_today().await;
    let id = engine
        .queue_mark(staff, block, today, MarkKind::In, Timestamp::now())
        .unwrap();
    assert_eq!(engine.status().pending_marks, 1);
    let report = engine.tick(Timestamp::now()).await.unwrap();
    assert!(report.data_changed);
    assert_eq!(engine.status().pending_marks, 0);
    assert_eq!(
        dev.i64(&format!("select count(*) from app.marks where id = '{id}'"))
            .await,
        1
    );
    assert!(engine.snapshot().unwrap().marks.iter().any(|m| m.id == id));

    // A schedule change on another device triggers a refetch and a new plan.
    api.schedule_set(
        &pc.device_secret,
        &session,
        staff,
        &week(&[(Weekday::Tuesday, t(10, 0), t(14, 0))]),
    )
    .await
    .unwrap();
    let report = engine.tick(Timestamp::now()).await.unwrap();
    assert!(report.data_changed);
    assert_eq!(engine.snapshot().unwrap().blocks.len(), 1);
    let v = api.get_version(&pc.device_secret).await.unwrap();
    assert_eq!(v.plan_config_version, v.config_version);

    // Revoked → reported as unpaired.
    let session = dev.login(&pc, &passphrase).await;
    api.device_revoke(&pc.device_secret, &session, pc.device_id)
        .await
        .unwrap();
    engine.tick(Timestamp::now()).await.unwrap();
    assert!(!engine.status().paired);
}

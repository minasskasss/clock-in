//! Phase 6 checks against dev that keep dev's data: they only read, or add
//! and remove their own throwaway rows, so devices paired to dev stay paired.
//!
//! ```text
//! cargo test -p clockin-sync --test backend keep:: -- --ignored --test-threads=1 --nocapture
//! ```

use super::{Dev, dev_as_is};
use uuid::Uuid;

/// The 20 RPCs of ARCHITECTURE §6, in name order.
const RPCS: [&str; 20] = [
    "admin_initialize",
    "admin_login",
    "admin_logout",
    "change_passphrase",
    "check_alarm",
    "device_revoke",
    "get_plan",
    "get_snapshot",
    "get_version",
    "mark",
    "mark_void",
    "override_delete",
    "override_set",
    "pair_device",
    "schedule_set",
    "settings_update",
    "staff_create",
    "staff_remove",
    "staff_update",
    "upload_plan",
];

/// The 11 tables of ARCHITECTURE §4, schema-qualified, in name order.
const TABLES: [&str; 11] = [
    "app.admin_sessions",
    "app.alarm_plan",
    "app.auth_state",
    "app.devices",
    "app.marks",
    "app.meta",
    "app.override_blocks",
    "app.overrides",
    "app.schedule_blocks",
    "app.settings",
    "app.staff",
];

async fn names(dev: &Dev, sql: &str) -> Vec<String> {
    dev.db
        .query(sql, &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect()
}

/// The Supabase Security Advisor's findings, rebuilt from its lint queries
/// (splinter): which ones fire, and that they are exactly the expected ones
/// (ARCHITECTURE §5). Prints them for the Phase 6 security review.
#[tokio::test]
#[ignore = "uses the dev Supabase project; keeps dev's data"]
async fn security_advisor_findings_are_the_expected_ones() {
    let dev = dev_as_is().await;

    // Lint 0028, "anon can execute a SECURITY DEFINER function": exactly the 20 RPCs.
    let definer = names(
        &dev,
        "select p.proname::text from pg_proc p join pg_namespace n on n.oid = p.pronamespace
          where n.nspname = 'public' and p.prosecdef
            and has_function_privilege('anon', p.oid, 'EXECUTE')
          order by 1",
    )
    .await;
    println!(
        "SECURITY DEFINER, callable by anon ({}): {}",
        definer.len(),
        definer.join(", ")
    );
    assert_eq!(definer, RPCS);

    // Lint 0029, the same for signed-in users: none.
    let signed_in = names(
        &dev,
        "select p.proname::text from pg_proc p join pg_namespace n on n.oid = p.pronamespace
          where n.nspname = 'public' and p.prosecdef
            and has_function_privilege('authenticated', p.oid, 'EXECUTE')",
    )
    .await;
    assert!(signed_in.is_empty(), "{signed_in:?}");

    // Lint 0008, "RLS enabled, no policy": exactly the 11 tables of schema app.
    let no_policy = names(
        &dev,
        "select n.nspname || '.' || c.relname from pg_class c
           join pg_namespace n on n.oid = c.relnamespace
          where c.relkind = 'r' and c.relrowsecurity
            and n.nspname in ('public', 'app')
            and not exists (select 1 from pg_policy pol where pol.polrelid = c.oid)
          order by 1",
    )
    .await;
    println!(
        "RLS on, no policy ({}): {}",
        no_policy.len(),
        no_policy.join(", ")
    );
    assert_eq!(no_policy, TABLES);

    // Lints that must not fire at all.
    let zero = [
        // 0013 "RLS disabled in public": there are no tables in public at all.
        (
            "tables or views in public",
            "select count(*) from pg_class c join pg_namespace n on n.oid = c.relnamespace
              where n.nspname = 'public' and c.relkind in ('r', 'p', 'v', 'm', 'f')",
        ),
        // 0010 "security definer view".
        (
            "views in app",
            "select count(*) from pg_class c join pg_namespace n on n.oid = c.relnamespace
              where n.nspname = 'app' and c.relkind in ('v', 'm')",
        ),
        // 0011 "function search_path mutable".
        (
            "functions without an empty search_path",
            "select count(*) from pg_proc p join pg_namespace n on n.oid = p.pronamespace
              where n.nspname in ('public', 'app')
                and not coalesce(p.proconfig @> array['search_path=\"\"'], false)",
        ),
        // 0014 "extension in public".
        (
            "extensions in public",
            "select count(*) from pg_extension e join pg_namespace n on n.oid = e.extnamespace
              where n.nspname = 'public'",
        ),
        // Supabase Auth is unused: nobody has signed up.
        ("Supabase Auth users", "select count(*) from auth.users"),
    ];
    for (what, sql) in zero {
        let n = dev.i64(sql).await;
        println!("{what}: {n}");
        assert_eq!(n, 0, "{what}");
    }
    assert_eq!(
        dev.text(
            "select n.nspname::text from pg_extension e join pg_namespace n on n.oid = e.extnamespace
              where e.extname = 'pgcrypto'"
        )
        .await,
        "extensions"
    );
}

/// The daily purge job (SPEC §9), run by hand on dev: it is scheduled and
/// has been running, and its own command deletes expired rows and keeps
/// recent ones. Uses throwaway rows (dev's own rows older than 30 days go
/// too, as they would at the next 01:17 UTC run).
#[tokio::test]
#[ignore = "uses the dev Supabase project; keeps dev's data"]
async fn purge_job_runs_on_dev() {
    let dev = dev_as_is().await;
    let job = dev
        .db
        .query_one(
            "select jobid, schedule, command, active from cron.job where jobname = 'clockin-purge'",
            &[],
        )
        .await
        .expect("the clockin-purge job is missing");
    let (jobid, schedule, command, active): (i64, String, String, bool) =
        (job.get(0), job.get(1), job.get(2), job.get(3));
    println!("job {jobid}: schedule '{schedule}', command '{command}', active {active}");
    assert_eq!(
        (schedule.as_str(), command.as_str(), active),
        ("17 1 * * *", "select app.purge()", true)
    );

    let runs = dev
        .db
        .query(
            "select to_char(start_time at time zone 'Europe/Athens', 'DD/MM/YYYY HH24:MI'), status
               from cron.job_run_details where jobid = $1 order by start_time desc limit 10",
            &[&jobid],
        )
        .await
        .unwrap();
    for r in &runs {
        println!(
            "scheduled run (Greek time): {} {}",
            r.get::<_, String>(0),
            r.get::<_, String>(1)
        );
    }
    assert!(!runs.is_empty(), "the job has never run");
    assert!(
        runs.iter().all(|r| r.get::<_, String>(1) == "succeeded"),
        "a scheduled run failed"
    );
    assert!(
        dev.boolean(&format!(
            "select max(start_time) > now() - interval '26 hours'
               from cron.job_run_details where jobid = {jobid}"
        ))
        .await,
        "no scheduled run in the last 26 hours"
    );

    // Throwaway rows: an active person with expired and recent marks and
    // one-off changes, and a person removed 4 days ago with no marks.
    let (kept, gone) = (Uuid::new_v4(), Uuid::new_v4());
    let (old_mark, new_mark) = (Uuid::new_v4(), Uuid::new_v4());
    dev.exec(&format!(
        "insert into app.staff (id, first_name, last_name) values ('{kept}', 'Δοκιμή', 'Καθαρισμού');
         insert into app.staff (id, first_name, last_name, removed_at)
           values ('{gone}', 'Παλιός', 'Καθαρισμού', now() - interval '4 days');
         insert into app.marks (id, staff_id, source_block_id, business_date, kind) values
           ('{old_mark}', '{kept}', gen_random_uuid(), app.business_today() - 31, 'in'),
           ('{new_mark}', '{kept}', gen_random_uuid(), app.business_today() - 29, 'in');
         insert into app.overrides (staff_id, business_date, kind) values
           ('{kept}', app.business_today() - 31, 'off'),
           ('{kept}', app.business_today() - 29, 'off');"
    ))
    .await;

    // The job's own command, as pg_cron runs it.
    let result: serde_json::Value = dev
        .db
        .query_one(command.as_str(), &[])
        .await
        .unwrap()
        .get(0);
    println!("manual run deleted: {result}");
    for key in ["marks", "overrides", "staff"] {
        assert!(result[key].as_i64().unwrap() >= 1, "{key}");
    }

    let dev = &dev;
    let rows = |sql: String| async move { dev.i64(&sql).await };
    assert_eq!(
        rows(format!(
            "select count(*) from app.marks where id = '{old_mark}'"
        ))
        .await,
        0,
        "the 31-day-old mark"
    );
    assert_eq!(
        rows(format!(
            "select count(*) from app.marks where id = '{new_mark}'"
        ))
        .await,
        1,
        "the 29-day-old mark"
    );
    assert_eq!(
        rows(format!(
            "select count(*) from app.overrides where staff_id = '{kept}'
              and business_date = app.business_today() - 29"
        ))
        .await,
        1,
        "the 29-day-old one-off change"
    );
    assert_eq!(
        rows(format!(
            "select count(*) from app.overrides where staff_id = '{kept}'"
        ))
        .await,
        1,
        "only the 29-day-old one-off change is left"
    );
    assert_eq!(
        rows(format!(
            "select count(*) from app.staff where id = '{gone}'"
        ))
        .await,
        0,
        "the person removed 4 days ago"
    );
    assert_eq!(
        rows(format!(
            "select count(*) from app.staff where id = '{kept}'"
        ))
        .await,
        1,
        "the active person"
    );

    // Remove the rest of the throwaway rows.
    dev.exec(&format!(
        "delete from app.marks where staff_id = '{kept}';
         delete from app.overrides where staff_id = '{kept}';
         delete from app.staff where id = '{kept}';"
    ))
    .await;
    assert_eq!(
        rows(format!(
            "select count(*) from app.staff where id in ('{kept}', '{gone}')"
        ))
        .await,
        0
    );
}

/// What dev holds right now (devices, people, upcoming alarms), unchanged.
#[tokio::test]
#[ignore = "uses the dev Supabase project; keeps dev's data"]
async fn dev_overview() {
    let dev = dev_as_is().await;
    for r in dev
        .db
        .query(
            "select name, platform,
                    coalesce(to_char(last_seen_at at time zone 'Europe/Athens', 'DD/MM/YYYY HH24:MI'), '-'),
                    revoked_at is not null
               from app.devices order by created_at",
            &[],
        )
        .await
        .unwrap()
    {
        println!(
            "device: {} ({}), last seen {}, revoked {}",
            r.get::<_, String>(0),
            r.get::<_, String>(1),
            r.get::<_, String>(2),
            r.get::<_, bool>(3)
        );
    }
    println!(
        "initialised {}, active staff {}, weekly blocks {}, upcoming one-off changes {}, marks {}, alarms ahead {}",
        dev.boolean("select passphrase_hash is not null from app.auth_state")
            .await,
        dev.i64("select count(*) from app.staff where removed_at is null")
            .await,
        dev.i64("select count(*) from app.schedule_blocks").await,
        dev.i64("select count(*) from app.overrides where business_date >= app.business_today()")
            .await,
        dev.i64("select count(*) from app.marks").await,
        dev.i64("select count(*) from app.alarm_plan where fires_at > now()")
            .await,
    );
    for r in dev
        .db
        .query(
            "select to_char(fires_at at time zone 'Europe/Athens', 'Dy DD/MM HH24:MI'), kind, display_name
               from app.alarm_plan where fires_at > now() order by fires_at limit 8",
            &[],
        )
        .await
        .unwrap()
    {
        println!(
            "next alarm: {} {} {}",
            r.get::<_, String>(0),
            r.get::<_, String>(1),
            r.get::<_, String>(2)
        );
    }
}

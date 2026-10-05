//! End-to-end check of the app layer against the **dev** Supabase project:
//! first-device setup, Settings, staff, marks, overrides, passphrase change
//! and self-revoke, all through [`AppState`] as the commands use it.
//!
//! It needs dev to be uninitialised and leaves it initialised with a
//! throwaway passphrase, so run it between two resets:
//!
//! ```text
//! cargo test -p clockin-sync --test backend reset_dev -- --ignored
//! cargo test -p clock-in e2e -- --ignored
//! cargo test -p clockin-sync --test backend reset_dev -- --ignored
//! ```

use crate::profile::Profile;
use crate::secrets::MemorySecrets;
use crate::state::{AppState, Phase};
use crate::views::TodayView;
use clockin_core::{MarkKind, OverrideKind, RowStatus, business_date_for};
use clockin_sync::{QuitCode, SettingsInput, WeekBlockInput};
use jiff::civil::Time;
use std::sync::Arc;
use std::time::Duration;

async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..100 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for: {what}");
}

fn today(state: &AppState) -> TodayView {
    state.today().unwrap().expect("no snapshot yet")
}

#[test]
#[ignore = "uses the dev Supabase project; reset dev before and after"]
fn e2e_against_dev() {
    assert_eq!(
        crate::config::ENVIRONMENT,
        "dev",
        "refusing to run against prod"
    );
    let config = crate::config::server_config().expect(".env.dev is missing");
    let dir = std::env::temp_dir().join(format!("clockin-e2e-{}", uuid::Uuid::new_v4()));

    tauri::async_runtime::block_on(async {
        let (state, secret) = AppState::open(
            Profile::default(),
            &dir,
            Box::new(MemorySecrets::default()),
            Some(config),
        )
        .unwrap();
        assert!(secret.is_none());
        let state = Arc::new(state);
        assert_eq!(state.phase(), Phase::Unpaired);

        // First device.
        let passphrase = crate::passgen::generate().unwrap();
        state
            .initialize(&passphrase, "1234", "E2E test PC")
            .await
            .expect("initialize failed: is dev reset? (see the module docs)");
        assert_eq!(state.phase(), Phase::Paired);
        eventually("first snapshot", || state.today().unwrap().is_some()).await;
        assert!(today(&state).rows.is_empty());

        // Settings.
        assert!(!state.admin_unlocked());
        let wrong = state
            .admin_login("abacus abacus abacus abacus abacus")
            .await;
        assert_eq!(wrong.unwrap_err().code(), Some("bad_passphrase"));
        state.admin_login(&passphrase).await.unwrap();
        assert!(state.admin_unlocked());

        // A split shift on today's business day, one part past midnight.
        let day = business_date_for(state.clock.now(), state.rollover());
        let blocks = vec![
            WeekBlockInput {
                id: None,
                weekday: day.weekday(),
                start: Time::constant(10, 0, 0, 0),
                end: Time::constant(14, 0, 0, 0),
            },
            WeekBlockInput {
                id: None,
                weekday: day.weekday(),
                start: Time::constant(19, 0, 0, 0),
                end: Time::constant(2, 0, 0, 0),
            },
        ];
        let staff_id = state
            .admin_call(|api, secret, session| async move {
                api.staff_create(&secret, &session, "Μαρία", "Παππά", &blocks)
                    .await
            })
            .await
            .unwrap();
        // admin_call waited for a sync round: the rows are there already.
        let rows = today(&state).rows;
        assert_eq!(rows.len(), 2);
        assert!(rows[1].end_next_day);
        let first = rows[0].clone();

        // Mark: shows at once, then reaches the server.
        state
            .mark(first.staff_id, first.source_block_id, day, MarkKind::In)
            .unwrap();
        assert!(matches!(
            today(&state).rows[0].status,
            RowStatus::CheckedIn | RowStatus::ShouldHaveLeft
        ));
        eventually("mark on the server", || {
            state
                .admin_view()
                .unwrap()
                .is_some_and(|v| v.marks.len() == 1)
        })
        .await;
        let mark_id = state.admin_view().unwrap().unwrap().marks[0].id;

        // Remove the mistaken mark.
        state
            .admin_call(|api, secret, session| async move {
                api.mark_void(&secret, &session, mark_id).await
            })
            .await
            .unwrap();
        assert!(matches!(
            today(&state).rows[0].status,
            RowStatus::Pending | RowStatus::Late
        ));

        // Day off today, then back.
        let override_id = state
            .admin_call(|api, secret, session| async move {
                api.override_set(&secret, &session, staff_id, day, OverrideKind::Off, &[])
                    .await
            })
            .await
            .unwrap();
        assert!(today(&state).rows.is_empty());
        state
            .admin_call(|api, secret, session| async move {
                api.override_delete(&secret, &session, override_id).await
            })
            .await
            .unwrap();
        assert_eq!(today(&state).rows.len(), 2);

        // New quit code.
        let settings = SettingsInput {
            checkin_offset_min: -5,
            checkout_offset_min: 0,
            rollover: state.rollover(),
            autostart: true,
            quit_code: QuitCode::new("9876".into()),
        };
        state
            .admin_call(|api, secret, session| async move {
                api.settings_update(&secret, &session, &settings).await
            })
            .await
            .unwrap();
        assert_eq!(state.current_quit_code().unwrap().expose(), "9876");

        // Change the passphrase: the session ends, the old one fails.
        let new_passphrase = crate::passgen::generate().unwrap();
        state
            .change_passphrase(&passphrase, &new_passphrase)
            .await
            .unwrap();
        let refused = state
            .admin_call(|api, secret, session| async move {
                api.staff_remove(&secret, &session, staff_id).await
            })
            .await;
        assert_eq!(refused.unwrap_err().code(), Some("no_session"));
        assert_eq!(
            state.admin_login(&passphrase).await.unwrap_err().code(),
            Some("bad_passphrase")
        );
        state.admin_login(&new_passphrase).await.unwrap();

        // Revoking this device returns it to the pairing screen.
        let this_device = state
            .admin_view()
            .unwrap()
            .unwrap()
            .devices
            .iter()
            .find(|d| d.this_device)
            .unwrap()
            .id;
        state
            .admin_call(|api, secret, session| async move {
                api.device_revoke(&secret, &session, this_device).await
            })
            .await
            .unwrap();
        eventually("unpaired", || state.phase() == Phase::Unpaired).await;
        assert!(state.today().unwrap().is_none());
        assert!(!state.admin_unlocked());
    });
    let _ = std::fs::remove_dir_all(&dir);
}

//! The commands the UI calls (`invoke`). They stay thin: rules live in
//! `clockin-core`, the server calls in `clockin-sync`, state in [`AppState`].

use crate::drafts::{
    self, OverrideReport, RangeDraft, WeekBlockDraft, WeekReport, check_override, check_week,
    parse_date, parse_hhmm,
};
use crate::error::CmdError;
use crate::passgen;
use crate::state::{AppState, Phase};
use crate::views::{AdminView, Banners, TodayView};
use clockin_core::{
    MarkKind, NameError, OverrideKind, ScheduleSettings, is_valid_quit_code, normalize_name,
    validate_settings,
};
use clockin_sync::{QuitCode, SettingsInput};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

type AppS<'a> = State<'a, Arc<AppState>>;

/// Everything the main screen needs, polled about once a second.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateView {
    phase: Phase,
    /// `dev` or `prod`.
    environment: &'static str,
    /// Debug builds only.
    debug: Option<DebugView>,
    /// Seconds left of a passphrase lockout (0 = none).
    lockout_remaining_s: u32,
    default_device_name: String,
    today: Option<TodayView>,
    banners: Banners,
    admin_unlocked: bool,
    data_version: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugView {
    profile: Option<String>,
    /// Greek wall clock "YYYY-MM-DDTHH:MM" while the fake clock is on.
    fake_clock: Option<String>,
}

#[tauri::command]
pub fn app_state(state: AppS<'_>) -> Result<AppStateView, CmdError> {
    let phase = state.phase();
    let paired = phase == Phase::Paired;
    Ok(AppStateView {
        phase,
        environment: crate::config::ENVIRONMENT,
        debug: cfg!(debug_assertions).then(|| DebugView {
            profile: state.profile().name().map(str::to_owned),
            fake_clock: state.clock.fake(),
        }),
        lockout_remaining_s: state.lockout_remaining_s(),
        default_device_name: state.profile().default_device_name(),
        today: if paired { state.today()? } else { None },
        banners: if paired {
            state.banners()
        } else {
            Banners::default()
        },
        admin_unlocked: paired && state.admin_unlocked(),
        data_version: state.data_version(),
    })
}

// --- First run --------------------------------------------------------------

/// Live feedback while typing a new passphrase. Returns nothing if fine.
#[tauri::command]
pub fn check_new_passphrase(passphrase: String) -> Result<(), CmdError> {
    clockin_core::check_passphrase(&passphrase)
        .map(drop)
        .map_err(crate::state::passphrase_error)
}

#[tauri::command]
pub async fn initialize(
    state: AppS<'_>,
    passphrase: String,
    quit_code: String,
    device_name: String,
) -> Result<(), CmdError> {
    state
        .initialize(&passphrase, &quit_code, &device_name)
        .await
}

#[tauri::command]
pub async fn pair(
    state: AppS<'_>,
    passphrase: String,
    device_name: String,
) -> Result<(), CmdError> {
    state.pair(&passphrase, &device_name).await
}

// --- Today --------------------------------------------------------------------

#[tauri::command]
pub fn mark(
    state: AppS<'_>,
    staff_id: Uuid,
    source_block_id: Uuid,
    business_date: String,
    kind: MarkKind,
) -> Result<(), CmdError> {
    let date = parse_date(&business_date).ok_or(CmdError::invalid("business_date", "format"))?;
    state.mark(staff_id, source_block_id, date, kind)
}

// --- Admin session --------------------------------------------------------------

#[tauri::command]
pub async fn admin_login(state: AppS<'_>, passphrase: String) -> Result<(), CmdError> {
    state.admin_login(&passphrase).await
}

#[tauri::command]
pub fn admin_logout(state: AppS<'_>) {
    state.admin_logout();
}

#[tauri::command]
pub fn admin_touch(state: AppS<'_>) {
    state.admin_touch();
}

#[tauri::command]
pub fn admin_view(state: AppS<'_>) -> Result<Option<AdminView>, CmdError> {
    state.admin_view()
}

// --- Editors' live checks -------------------------------------------------------

#[tauri::command]
pub fn validate_week(state: AppS<'_>, blocks: Vec<WeekBlockDraft>) -> WeekReport {
    check_week(&blocks, state.rollover())
}

#[tauri::command]
pub fn validate_override(
    state: AppS<'_>,
    kind: OverrideKind,
    business_date: String,
    blocks: Vec<RangeDraft>,
) -> OverrideReport {
    check_override(
        kind,
        &business_date,
        &blocks,
        state.rollover(),
        state.clock.now(),
    )
}

// --- Admin changes ----------------------------------------------------------------

fn name(field: &'static str, raw: &str) -> Result<String, CmdError> {
    normalize_name(raw).map_err(|e| {
        let (problem, character) = match e {
            NameError::Empty => ("empty", None),
            NameError::TooLong => ("too_long", None),
            NameError::NoLetters => ("no_letters", None),
            NameError::InvalidCharacter(c) => ("invalid_character", Some(c)),
        };
        CmdError::Invalid {
            field,
            problem: problem.into(),
            positions: None,
            character,
        }
    })
}

/// Creates a staff member (`id` = None) or saves names and weekly schedule.
#[tauri::command]
pub async fn staff_save(
    state: AppS<'_>,
    id: Option<Uuid>,
    first_name: String,
    last_name: String,
    blocks: Vec<WeekBlockDraft>,
) -> Result<(), CmdError> {
    let first = name("first_name", &first_name)?;
    let last = name("last_name", &last_name)?;
    if !check_week(&blocks, state.rollover()).ok {
        return Err(CmdError::invalid("blocks", "invalid"));
    }
    let inputs = drafts::week_inputs(&blocks).ok_or(CmdError::invalid("blocks", "invalid"))?;
    match id {
        None => {
            state
                .admin_call(|api, secret, session| async move {
                    api.staff_create(&secret, &session, &first, &last, &inputs)
                        .await
                        .map(drop)
                })
                .await
        }
        Some(id) => {
            state
                .admin_call(|api, secret, session| async move {
                    api.staff_update(&secret, &session, id, &first, &last)
                        .await?;
                    api.schedule_set(&secret, &session, id, &inputs).await
                })
                .await
        }
    }
}

#[tauri::command]
pub async fn staff_remove(state: AppS<'_>, id: Uuid) -> Result<(), CmdError> {
    state
        .admin_call(
            |api, secret, session| async move { api.staff_remove(&secret, &session, id).await },
        )
        .await
}

#[tauri::command]
pub async fn override_save(
    state: AppS<'_>,
    staff_id: Uuid,
    business_date: String,
    kind: OverrideKind,
    blocks: Vec<RangeDraft>,
) -> Result<(), CmdError> {
    let report = check_override(
        kind,
        &business_date,
        &blocks,
        state.rollover(),
        state.clock.now(),
    );
    if let Some(problem) = report.date_problem {
        return Err(CmdError::invalid("business_date", problem));
    }
    if !report.ok {
        return Err(CmdError::invalid("blocks", "invalid"));
    }
    let date = parse_date(&business_date).ok_or(CmdError::invalid("business_date", "bad_date"))?;
    let inputs = match kind {
        OverrideKind::Off => Vec::new(),
        OverrideKind::Replace => {
            drafts::override_inputs(&blocks).ok_or(CmdError::invalid("blocks", "invalid"))?
        }
    };
    state
        .admin_call(|api, secret, session| async move {
            api.override_set(&secret, &session, staff_id, date, kind, &inputs)
                .await
                .map(drop)
        })
        .await
}

#[tauri::command]
pub async fn override_delete(state: AppS<'_>, id: Uuid) -> Result<(), CmdError> {
    state
        .admin_call(|api, secret, session| async move {
            api.override_delete(&secret, &session, id).await.map(drop)
        })
        .await
}

/// Saves offsets, rollover, autostart and (if given) a new quit code.
#[tauri::command]
pub async fn settings_save(
    state: AppS<'_>,
    checkin_offset_min: i32,
    checkout_offset_min: i32,
    rollover: String,
    autostart: bool,
    quit_code: Option<String>,
) -> Result<(), CmdError> {
    let rollover = parse_hhmm(&rollover).ok_or(CmdError::invalid("rollover", "format"))?;
    let schedule = ScheduleSettings {
        checkin_offset_min,
        checkout_offset_min,
        rollover,
    };
    if let Err(problems) = validate_settings(&schedule) {
        let field = match problems.first() {
            Some(clockin_core::SettingsProblem::CheckinOffset) => "checkin_offset_min",
            Some(clockin_core::SettingsProblem::CheckoutOffset) => "checkout_offset_min",
            _ => "rollover",
        };
        return Err(CmdError::invalid(field, "range"));
    }
    let quit_code = match quit_code.filter(|c| !c.is_empty()) {
        Some(code) if is_valid_quit_code(&code) => QuitCode::new(code),
        Some(_) => return Err(CmdError::invalid("quit_code", "format")),
        None => state
            .current_quit_code()
            .ok_or(CmdError::invalid("quit_code", "missing"))?,
    };
    let input = SettingsInput {
        checkin_offset_min,
        checkout_offset_min,
        rollover,
        autostart,
        quit_code,
    };
    state
        .admin_call(|api, secret, session| async move {
            api.settings_update(&secret, &session, &input).await
        })
        .await
}

#[tauri::command]
pub async fn mark_void(state: AppS<'_>, id: Uuid) -> Result<(), CmdError> {
    state
        .admin_call(|api, secret, session| async move {
            api.mark_void(&secret, &session, id).await.map(drop)
        })
        .await
}

#[tauri::command]
pub async fn device_revoke(state: AppS<'_>, id: Uuid) -> Result<(), CmdError> {
    state
        .admin_call(
            |api, secret, session| async move { api.device_revoke(&secret, &session, id).await },
        )
        .await
}

#[tauri::command]
pub async fn change_passphrase(
    state: AppS<'_>,
    current: String,
    new_passphrase: String,
) -> Result<(), CmdError> {
    state.change_passphrase(&current, &new_passphrase).await
}

/// A fresh 5-word passphrase for the Change passphrase form. Only while
/// Settings are unlocked.
#[tauri::command]
pub fn generate_passphrase(state: AppS<'_>) -> Result<String, CmdError> {
    if !state.admin_unlocked() {
        return Err(CmdError::Rejected {
            code: "no_session".into(),
            retry_after_s: None,
            details: None,
        });
    }
    passgen::generate().map_err(CmdError::internal)
}

// --- Debug ----------------------------------------------------------------------

/// Debug builds: sets the fake clock to a Greek wall-clock time
/// ("YYYY-MM-DDTHH:MM"), or back to real time with `null`.
#[tauri::command]
pub fn debug_set_clock(state: AppS<'_>, local: Option<String>) -> Result<(), CmdError> {
    let target = match local {
        None => None,
        Some(text) => Some(
            jiff::civil::DateTime::strptime("%Y-%m-%dT%H:%M", text.trim())
                .map_err(|_| CmdError::invalid("fake_clock", "format"))?,
        ),
    };
    if state.set_fake_clock(target) {
        Ok(())
    } else {
        Err(CmdError::invalid("fake_clock", "unavailable"))
    }
}

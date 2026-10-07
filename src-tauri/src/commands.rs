//! The commands the UI calls (`invoke`). They stay thin: rules live in
//! `clockin-core`, the server calls in `clockin-sync`, state in [`AppState`].

#[cfg(desktop)]
use crate::alarms::{AlarmView, Alarms};
use crate::drafts::{
    self, OverrideReport, RangeDraft, WeekBlockDraft, WeekReport, check_override, check_week,
    parse_date, parse_hhmm,
};
use crate::error::CmdError;
use crate::passgen;
use crate::state::{AlertMode, AppState, Phase, ThemePreference};
use crate::views::{AdminView, Banners, TodayView};
use clockin_core::{
    MarkKind, NameError, OverrideKind, ScheduleSettings, is_valid_quit_code, normalize_name,
    same_passphrase, validate_settings,
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
    /// Whether the automatic theme is dark right now (SPEC §3).
    auto_dark: bool,
    /// Whether Quit asks for a quit code (false before one is known).
    quit_code_set: bool,
    /// `windows` or `android`.
    platform: &'static str,
    /// Android only.
    android: Option<AndroidView>,
}

/// Android: this phone's alert mode and permission checklist (SPEC §8.2).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidView {
    alert_mode: AlertMode,
    /// `None` until the checklist was first read.
    permissions: Option<tauri_plugin_clockin_alarm::PermissionStatus>,
    /// Everything the alarms need is granted (the Today banner, SPEC §6).
    permissions_ok: bool,
    /// A ring-mode alarm in progress on this phone: Today shows «Σταμάτημα».
    alarm: Option<AlarmBanner>,
}

/// The ringing (or silent-between-rings) alarm, for the bar on Today.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmBanner {
    ringing: bool,
    check_in: Vec<String>,
    check_out: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugView {
    profile: Option<String>,
    /// Greek wall clock "YYYY-MM-DDTHH:MM" while the fake clock is on.
    fake_clock: Option<String>,
    /// The fake time is in the second pass of the repeated hour.
    fake_clock_second: bool,
}

#[tauri::command]
#[cfg_attr(
    not(target_os = "android"),
    allow(unused_variables, clippy::needless_pass_by_value)
)]
pub fn app_state(app: tauri::AppHandle, state: AppS<'_>) -> Result<AppStateView, CmdError> {
    let phase = state.phase();
    let paired = phase == Phase::Paired;
    Ok(AppStateView {
        phase,
        environment: crate::config::ENVIRONMENT,
        debug: cfg!(debug_assertions).then(|| DebugView {
            profile: state.profile().name().map(str::to_owned),
            fake_clock: state.clock.fake(),
            fake_clock_second: state.clock.fake_second_pass(),
        }),
        lockout_remaining_s: state.lockout_remaining_s(),
        default_device_name: state.default_device_name(),
        today: if paired { state.today()? } else { None },
        banners: if paired {
            state.banners()
        } else {
            Banners::default()
        },
        admin_unlocked: paired && state.admin_unlocked(),
        data_version: state.data_version(),
        auto_dark: clockin_core::auto_theme_is_dark(state.clock.now(), state.rollover()),
        quit_code_set: state.quit_code_set(),
        platform: if cfg!(target_os = "android") {
            "android"
        } else {
            "windows"
        },
        android: android_view(&app, &state),
    })
}

#[cfg(target_os = "android")]
fn android_view(app: &tauri::AppHandle, state: &AppState) -> Option<AndroidView> {
    use tauri::Manager;
    let android = app.try_state::<Arc<crate::android::Android>>();
    let permissions = android.as_ref().and_then(|a| a.status());
    let alarm = android
        .map(|a| a.alarm())
        .filter(|a| a.active)
        .map(|a| AlarmBanner {
            ringing: a.ringing,
            check_in: a.check_in,
            check_out: a.check_out,
        });
    Some(AndroidView {
        alert_mode: state.alert_mode(),
        permissions_ok: permissions
            .as_ref()
            .is_none_or(tauri_plugin_clockin_alarm::PermissionStatus::all_granted),
        permissions,
        alarm,
    })
}

#[cfg(not(target_os = "android"))]
fn android_view(_app: &tauri::AppHandle, _state: &AppState) -> Option<AndroidView> {
    None
}

// --- Android: alert mode and the permission checklist -------------------------------

/// Ring or Notification, per device, no passphrase (SPEC §8.2).
#[tauri::command]
#[cfg_attr(
    not(target_os = "android"),
    allow(unused_variables, clippy::needless_pass_by_value)
)]
pub fn set_alert_mode(
    app: tauri::AppHandle,
    state: AppS<'_>,
    mode: AlertMode,
) -> Result<(), CmdError> {
    state.set_alert_mode(mode)?;
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        if let Some(android) = app.try_state::<Arc<crate::android::Android>>() {
            android.wake.notify_one();
        }
    }
    Ok(())
}

/// This device's theme choice (SPEC §3), reported by the UI at start and on
/// every change. Android's native alarm screen follows it.
#[tauri::command]
#[cfg_attr(
    not(target_os = "android"),
    allow(unused_variables, clippy::needless_pass_by_value)
)]
pub fn set_theme(
    app: tauri::AppHandle,
    state: AppS<'_>,
    theme: ThemePreference,
) -> Result<(), CmdError> {
    state.set_theme(theme)?;
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        if let Some(android) = app.try_state::<Arc<crate::android::Android>>() {
            android.wake.notify_one();
        }
    }
    Ok(())
}

/// «Σταμάτημα» on the Today bar: ends the ring-mode alarm cycle on this
/// phone. Like the alarm screen's button, it marks nobody (SPEC §7.3).
#[tauri::command]
#[cfg_attr(
    not(target_os = "android"),
    allow(unused_variables, clippy::unused_async)
)]
pub async fn android_stop_alarm(app: tauri::AppHandle) -> Result<(), CmdError> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        if let Some(Err(e)) = crate::android::bridge(&app, |b| b.stop_alarm()).await {
            return Err(CmdError::internal(e));
        }
        if let Some(Ok(alarm)) = crate::android::bridge(&app, |b| b.alarm_status()).await
            && let Some(android) = app.try_state::<Arc<crate::android::Android>>()
        {
            android.set_alarm(alarm);
        }
    }
    Ok(())
}

/// Opens the Android screen that fixes one checklist item (`notifications`,
/// `exactAlarms`, `fullScreen`, `battery`, `unusedApps`, `oem`, and the
/// Xiaomi steps `xiaomiAutostart`, `xiaomiPermissions`, `xiaomiBattery`).
#[tauri::command]
#[cfg_attr(
    not(target_os = "android"),
    allow(unused_variables, clippy::unused_async)
)]
pub async fn android_open_settings(app: tauri::AppHandle, kind: String) -> Result<(), CmdError> {
    #[cfg(target_os = "android")]
    if let Some(Err(e)) = crate::android::bridge(&app, move |b| b.open_settings(&kind)).await {
        return Err(CmdError::internal(e));
    }
    Ok(())
}

/// The phone maker's extra step was done (it can't be checked automatically).
#[tauri::command]
#[cfg_attr(
    not(target_os = "android"),
    allow(unused_variables, clippy::unused_async)
)]
pub async fn android_set_oem_done(app: tauri::AppHandle, done: bool) -> Result<(), CmdError> {
    #[cfg(target_os = "android")]
    if let Some(Err(e)) = crate::android::bridge(&app, move |b| b.set_oem_done(done)).await {
        return Err(CmdError::internal(e));
    }
    Ok(())
}

/// The read-only «Διαγνωστικά» view (Android): enough to set up and check a
/// phone remotely from one screenshot. No secrets.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsView {
    app_version: &'static str,
    environment: &'static str,
    phone: String,
    android_version: String,
    sdk: i32,
    /// MIUI or HyperOS version, or "".
    maker_os: String,
    alert_mode: AlertMode,
    permissions: Option<tauri_plugin_clockin_alarm::PermissionStatus>,
    /// The app's own last sync with the server (while it was open).
    last_sync: Option<crate::views::LocalStamp>,
    /// The background plan refresh (every 15 minutes, app closed).
    last_refresh: Option<crate::views::LocalStamp>,
    last_refresh_ok: Option<bool>,
    next_alarm: Option<crate::views::LocalStamp>,
    last_alarm: Option<crate::views::LocalStamp>,
    /// `fullScreen`, `opened`, `notification` or `notificationMode`.
    last_alarm_how: Option<String>,
}

/// # Errors
///
/// If Android can't be asked.
#[tauri::command]
#[cfg_attr(
    not(target_os = "android"),
    allow(unused_variables, clippy::unused_async)
)]
pub async fn android_diagnostics(
    app: tauri::AppHandle,
    state: AppS<'_>,
) -> Result<Option<DiagnosticsView>, CmdError> {
    #[cfg(target_os = "android")]
    let view = {
        use crate::views::LocalStamp;
        use tauri::Manager;
        let d = match crate::android::bridge(&app, |b| b.diagnostics()).await {
            Some(Ok(d)) => d,
            Some(Err(e)) => return Err(CmdError::internal(e)),
            None => return Ok(None),
        };
        let stamp = |ms: Option<i64>| {
            ms.and_then(|ms| jiff::Timestamp::from_millisecond(ms).ok())
                .map(LocalStamp::at)
        };
        let permissions = app
            .try_state::<Arc<crate::android::Android>>()
            .and_then(|a| a.status());
        let paired = state.phase() == Phase::Paired;
        Some(DiagnosticsView {
            app_version: env!("CARGO_PKG_VERSION"),
            environment: crate::config::ENVIRONMENT,
            phone: format!("{} {}", d.manufacturer, d.model).trim().to_owned(),
            android_version: d.android_version,
            sdk: d.sdk,
            maker_os: d.maker_os,
            alert_mode: state.alert_mode(),
            permissions,
            last_sync: if paired {
                state.banners().last_sync
            } else {
                None
            },
            last_refresh: stamp(d.last_refresh_at),
            last_refresh_ok: d.last_refresh_ok,
            next_alarm: stamp(d.next_alarm_at),
            last_alarm: stamp(d.last_alarm_at),
            last_alarm_how: d.last_alarm_how,
        })
    };
    #[cfg(not(target_os = "android"))]
    let view = None;
    Ok(view)
}

// --- First run --------------------------------------------------------------

/// Live feedback while typing a new passphrase. Returns nothing if fine.
#[tauri::command]
pub fn check_new_passphrase(passphrase: String, current: Option<String>) -> Result<(), CmdError> {
    clockin_core::check_passphrase(&passphrase).map_err(crate::state::passphrase_error)?;
    // Both typed into the same form: nothing is learned from the answer.
    match current {
        Some(current) if same_passphrase(&current, &passphrase) => {
            Err(CmdError::invalid("passphrase", "same_as_current"))
        }
        _ => Ok(()),
    }
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

/// «Εντάξει» on the notice for a mark the server refused.
#[tauri::command]
pub fn dismiss_refused_mark(state: AppS<'_>, id: Uuid) -> Result<(), CmdError> {
    state.dismiss_refused_mark(id)
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
    let quit_code = new_quit_code(quit_code, state.current_quit_code(), state.admin_unlocked())?;
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

/// The quit code a settings save sends: the requested new one, or the current
/// one when none is given. A new code equal to the current one is refused.
fn new_quit_code(
    requested: Option<String>,
    current: Option<QuitCode>,
    unlocked: bool,
) -> Result<QuitCode, CmdError> {
    match requested.filter(|c| !c.is_empty()) {
        // Only with Settings open, so the answer can't be used to guess the code.
        Some(_) if !unlocked => Err(crate::state::no_session()),
        Some(code) if current.as_ref().is_some_and(|c| c.expose() == code) => {
            Err(CmdError::invalid("quit_code", "same_as_current"))
        }
        Some(code) if is_valid_quit_code(&code) => Ok(QuitCode::new(code)),
        Some(_) => Err(CmdError::invalid("quit_code", "format")),
        None => current.ok_or(CmdError::invalid("quit_code", "missing")),
    }
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

// --- Alarms and quitting (Windows) ------------------------------------------------

/// What the alarm window polls.
#[cfg(desktop)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmStateView {
    /// `None` once the alarm ended (the window is about to close).
    alarm: Option<AlarmView>,
    auto_dark: bool,
}

#[cfg(desktop)]
#[tauri::command]
pub fn alarm_state(state: AppS<'_>, alarms: State<'_, Arc<Alarms>>) -> AlarmStateView {
    AlarmStateView {
        alarm: alarms.view(),
        auto_dark: clockin_core::auto_theme_is_dark(state.clock.now(), state.rollover()),
    }
}

/// Stop on the alarm window: ends alarm `id` on this device only. It marks
/// nobody (SPEC §7.3).
#[cfg(desktop)]
#[tauri::command]
pub fn alarm_stop(state: AppS<'_>, alarms: State<'_, Arc<Alarms>>, id: u64) {
    alarms.stop(&state, id);
}

/// Tray → Quit (SPEC §8.1): exits only with the right quit code, checked on
/// this device (works offline). No lockout: the code only prevents closing
/// by accident.
#[cfg(desktop)]
#[tauri::command]
pub fn quit(app: tauri::AppHandle, state: AppS<'_>, code: String) -> Result<(), CmdError> {
    if state.quit_allowed(&code) {
        app.exit(0);
        Ok(())
    } else {
        Err(CmdError::invalid("quit_code", "wrong"))
    }
}

// --- Debug ----------------------------------------------------------------------

/// Debug builds: sets the fake clock to a Greek wall-clock time
/// ("YYYY-MM-DDTHH:MM"), or back to real time with `null`. `second`: the
/// second pass of the hour that repeats when the clocks go back.
#[tauri::command]
pub fn debug_set_clock(
    state: AppS<'_>,
    local: Option<String>,
    second: Option<bool>,
) -> Result<(), CmdError> {
    let target = match local {
        None => None,
        // ISO "2026-10-05T21:00"; the UI reads the typed dd/mm/yyyy HH:MM.
        Some(text) => Some(
            jiff::civil::DateTime::strptime("%Y-%m-%dT%H:%M", text.trim())
                .map_err(|_| CmdError::invalid("fake_clock", "format"))?,
        ),
    };
    if state.set_fake_clock(target, second.unwrap_or(false)) {
        Ok(())
    } else {
        Err(CmdError::invalid("fake_clock", "unavailable"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "abacus zoom cloud tiger mango";

    #[test]
    fn a_new_passphrase_equal_to_the_current_one_is_refused() {
        assert_eq!(check_new_passphrase(GOOD.into(), None), Ok(()));
        assert_eq!(
            check_new_passphrase(GOOD.into(), Some("abacus zoom cloud tiger melon".into())),
            Ok(())
        );
        assert_eq!(
            check_new_passphrase(
                GOOD.into(),
                Some("  Abacus ZOOM cloud  tiger mango ".into())
            ),
            Err(CmdError::invalid("passphrase", "same_as_current"))
        );
        // A badly formed new passphrase reports that first.
        assert_eq!(
            check_new_passphrase("abacus".into(), Some("abacus".into())),
            Err(CmdError::invalid("passphrase", "too_few_words"))
        );
    }

    #[test]
    fn a_new_quit_code_equal_to_the_current_one_is_refused() {
        let current = || Some(QuitCode::new("1234".into()));
        assert_eq!(
            new_quit_code(Some("1234".into()), current(), true),
            Err(CmdError::invalid("quit_code", "same_as_current"))
        );
        assert_eq!(
            new_quit_code(Some("5678".into()), current(), true)
                .unwrap()
                .expose(),
            "5678"
        );
        assert_eq!(
            new_quit_code(Some("12a4".into()), current(), true),
            Err(CmdError::invalid("quit_code", "format"))
        );
        // No new code: the current one is kept.
        assert_eq!(
            new_quit_code(None, current(), true).unwrap().expose(),
            "1234"
        );
        assert_eq!(
            new_quit_code(Some(String::new()), current(), true)
                .unwrap()
                .expose(),
            "1234"
        );
        // Locked Settings: no answer either way.
        assert_eq!(
            new_quit_code(Some("1234".into()), current(), false),
            Err(crate::state::no_session())
        );
        assert_eq!(
            new_quit_code(Some("5678".into()), current(), false),
            Err(crate::state::no_session())
        );
    }
}

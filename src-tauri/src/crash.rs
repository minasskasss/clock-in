//! Crash records (PLAN Phase 6, DECISIONS 2026-10-07).
//!
//! - **Desktop:** every panic appends one line to `crash.log` in the app's
//!   data folder: the Greek time, version, dev/prod, thread, the panic
//!   message and its file:line. No backtrace, data, codes or secrets. The
//!   restart watcher (`watcher.rs`) appends its own lines to the same file.
//!   The file stays under about 100 KB: when it would grow past that, its
//!   older half is dropped.
//! - **Android:** the last panic is kept in `last-crash.json` for
//!   «Διαγνωστικά» (Kotlin keeps its own last crash; the newer one is shown).
//!
//! Lines are technical English, like any log: they are read by whoever
//! supports the app, not shown in the app.

use jiff::Timestamp;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};

/// The desktop crash log, in the app's data folder.
pub const LOG_FILE: &str = "crash.log";

/// `crash.log` is kept under this size.
pub const MAX_LOG_BYTES: usize = 100 * 1024;

/// Android: the last panic.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub const LAST_FILE: &str = "last-crash.json";

/// Panic messages are cut to this many characters.
const MAX_MESSAGE_CHARS: usize = 300;

/// Records every panic in `dir` (then runs the previous hook, which prints
/// it in debug builds). Panics never stop here: with `panic = "abort"` the
/// process ends right after.
pub fn install(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let error = describe(info);
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_owned();
        #[cfg(not(target_os = "android"))]
        let _ = append_line(
            &dir.join(LOG_FILE),
            &line(Timestamp::now(), &format!("thread {thread}"), &error),
        );
        #[cfg(target_os = "android")]
        {
            let _ = thread;
            let _ = save_last(&dir, Timestamp::now(), &error);
        }
        previous(info);
    }));
}

/// "panicked at src\state.rs:12:5: <message>", on one line.
fn describe(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(no message)");
    let location = info.location().map_or_else(
        || "unknown place".to_owned(),
        |l| format!("{}:{}:{}", l.file(), l.line(), l.column()),
    );
    format!("panicked at {location}: {}", one_line(message))
}

/// Newlines become spaces, and long text is cut.
fn one_line(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut cut: String = flat.chars().take(MAX_MESSAGE_CHARS).collect();
    if flat.chars().count() > MAX_MESSAGE_CHARS {
        cut.push('…');
    }
    cut
}

/// One `crash.log` line: "09/10/2026 14:03:12 | Clock In 1.0.0 (prod) | who | what".
#[must_use]
pub fn line(at: Timestamp, who: &str, what: &str) -> String {
    let local = clockin_core::shop_datetime(at);
    format!(
        "{} | Clock In {} ({}) | {who} | {}",
        local.strftime("%d/%m/%Y %H:%M:%S"),
        env!("CARGO_PKG_VERSION"),
        crate::config::ENVIRONMENT,
        one_line(what),
    )
}

/// Appends `line` to the log at `path`, dropping the older half first if
/// the file would grow past [`MAX_LOG_BYTES`].
///
/// # Errors
///
/// If the file can't be read or written.
pub fn append_line(path: &Path, line: &str) -> io::Result<()> {
    let entry = format!("{line}\n");
    let size = fs::metadata(path).map_or(0, |m| usize::try_from(m.len()).unwrap_or(usize::MAX));
    if size.saturating_add(entry.len()) <= MAX_LOG_BYTES {
        return OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(entry.as_bytes());
    }
    let old = fs::read(path)?;
    // Keep the newest half, starting at a whole line.
    let from = old.len().saturating_sub(MAX_LOG_BYTES / 2);
    let start = if from == 0 {
        0
    } else {
        old[from..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(old.len(), |p| from + p + 1)
    };
    let mut kept = old[start..].to_vec();
    kept.extend_from_slice(entry.as_bytes());
    fs::write(path, kept)
}

/// Android: the last crash, for «Διαγνωστικά».
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastCrash {
    /// Milliseconds since 1970.
    pub at_ms: i64,
    pub version: String,
    pub error: String,
}

/// Android: replaces the last crash record.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn save_last(dir: &Path, at: Timestamp, error: &str) -> io::Result<()> {
    let record = LastCrash {
        at_ms: at.as_millisecond(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        error: one_line(error),
    };
    let json = serde_json::to_vec(&record).map_err(io::Error::other)?;
    fs::write(dir.join(LAST_FILE), json)
}

/// Android: the last panic recorded in `dir`, if any.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[must_use]
pub fn load_last(dir: &Path) -> Option<LastCrash> {
    let bytes = fs::read(dir.join(LAST_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("clockin-crash-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_line_has_greek_time_version_environment_and_one_line_text() {
        // 11:03:12 UTC = 14:03:12 in Greece (summer time).
        let at: Timestamp = "2026-10-09T11:03:12Z".parse().unwrap();
        let line = line(at, "thread main", "first\nsecond");
        assert_eq!(
            line,
            format!(
                "09/10/2026 14:03:12 | Clock In {} ({}) | thread main | first second",
                env!("CARGO_PKG_VERSION"),
                crate::config::ENVIRONMENT
            )
        );
    }

    #[test]
    fn long_messages_are_cut() {
        let text = one_line(&"x".repeat(1000));
        assert_eq!(text.chars().count(), MAX_MESSAGE_CHARS + 1);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn the_log_stays_under_its_cap_and_keeps_the_newest_lines() {
        let dir = temp_dir("cap");
        let path = dir.join(LOG_FILE);
        for i in 0..5000 {
            append_line(&path, &format!("line {i:05} {}", "y".repeat(60))).unwrap();
            assert!(fs::metadata(&path).unwrap().len() <= MAX_LOG_BYTES as u64);
        }
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.ends_with(&format!("line 04999 {}\n", "y".repeat(60))));
        // Whole lines only, in order.
        let numbers: Vec<u32> = text.lines().map(|l| l[5..10].parse().unwrap()).collect();
        assert!(numbers.windows(2).all(|w| w[1] == w[0] + 1));
        assert!(numbers.len() > 500, "about half the cap is kept");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_panic_is_recorded_with_its_location() {
        let dir = temp_dir("hook");
        let previous = std::panic::take_hook();
        install(dir.clone());
        let result = std::thread::Builder::new()
            .name("crash-test".into())
            .spawn(|| panic!("deliberate test panic"))
            .unwrap()
            .join();
        std::panic::set_hook(previous);
        assert!(result.is_err());
        let text = fs::read_to_string(dir.join(LOG_FILE)).unwrap();
        // Other tests may panic meanwhile; find this one's line.
        let line = text
            .lines()
            .find(|l| l.contains("| thread crash-test | "))
            .unwrap_or_else(|| panic!("no line for the test thread: {text}"));
        assert!(line.contains("panicked at "), "{line}");
        assert!(line.contains("crash.rs:"), "{line}");
        assert!(line.ends_with(": deliberate test panic"), "{line}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_android_record_round_trips() {
        let dir = temp_dir("last");
        assert_eq!(load_last(&dir), None);
        let at = Timestamp::from_millisecond(1_800_000_000_000).unwrap();
        save_last(&dir, at, "panicked at a.rs:1:1: boom").unwrap();
        let last = load_last(&dir).unwrap();
        assert_eq!(last.at_ms, 1_800_000_000_000);
        assert_eq!(last.error, "panicked at a.rs:1:1: boom");
        assert_eq!(last.version, env!("CARGO_PKG_VERSION"));
        fs::remove_dir_all(dir).unwrap();
    }
}

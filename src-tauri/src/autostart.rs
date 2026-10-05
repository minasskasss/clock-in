//! Windows: "Start with Windows" (SPEC §4.4, §8.1). A value in the current
//! user's `Run` key starts Clock In at login, into the tray (`--autostart`).
//!
//! Written here rather than with `tauri-plugin-autostart`, which stores the
//! program path without quotes: Clock In installs into a folder with a space
//! in its name (`…\Clock In\`), and an unquoted path with spaces is parsed
//! ambiguously by Windows.
//!
//! Only release builds of the default profile touch the registry, so a
//! development build never registers itself.

/// The command-line flag the `Run` entry passes.
pub const FLAG: &str = "--autostart";

/// Whether this process was started by the `Run` entry.
#[must_use]
pub fn started_at_login() -> bool {
    std::env::args().skip(1).any(|a| a == FLAG)
}

/// The `Run` value for the program at `exe`: the quoted path plus the flag.
#[must_use]
pub fn command_line(exe: &std::path::Path) -> String {
    format!("\"{}\" {FLAG}", exe.display())
}

#[cfg(windows)]
pub use registry::apply;

#[cfg(windows)]
mod registry {
    use super::command_line;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// The value name, shown in Task Manager's Startup apps.
    const VALUE: &str = "Clock In";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The current `Run` value, if there is one.
    #[allow(unsafe_code)]
    fn read() -> Option<String> {
        let (key, value) = (wide(RUN_KEY), wide(VALUE));
        let mut buf = vec![0u16; 1024];
        let mut bytes = u32::try_from(buf.len() * 2).ok()?;
        // SAFETY: null-terminated UTF-16 names; `buf` holds `bytes` bytes and
        // RegGetValueW writes at most that many (it fails if more are needed).
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &raw mut bytes,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = (bytes as usize / 2).saturating_sub(1).min(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    /// Makes the `Run` entry match the setting (adding, fixing or removing
    /// it). Task Manager's own on/off switch for it is left alone.
    #[allow(unsafe_code)]
    pub fn apply(enabled: bool) {
        if cfg!(debug_assertions) {
            return;
        }
        let (key, value) = (wide(RUN_KEY), wide(VALUE));
        if enabled {
            let Ok(exe) = std::env::current_exe() else {
                return;
            };
            let wanted = command_line(&exe);
            if read().as_deref() == Some(wanted.as_str()) {
                return;
            }
            let data = wide(&wanted);
            let Ok(size) = u32::try_from(data.len() * 2) else {
                return;
            };
            // SAFETY: null-terminated UTF-16 names and data of `size` bytes.
            let status = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    value.as_ptr(),
                    REG_SZ,
                    data.as_ptr().cast(),
                    size,
                )
            };
            if status != ERROR_SUCCESS {
                eprintln!("clock-in: could not turn on start with Windows ({status})");
            }
        } else if read().is_some() {
            // SAFETY: null-terminated UTF-16 names.
            let status =
                unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
            if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
                eprintln!("clock-in: could not turn off start with Windows ({status})");
            }
        }
    }
}

/// Other desktops: nothing to do (development only).
#[cfg(not(windows))]
pub fn apply(_enabled: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_quoted() {
        let exe = std::path::Path::new(r"C:\Users\Shop PC\AppData\Local\Clock In\clock-in.exe");
        assert_eq!(
            command_line(exe),
            r#""C:\Users\Shop PC\AppData\Local\Clock In\clock-in.exe" --autostart"#
        );
    }
}

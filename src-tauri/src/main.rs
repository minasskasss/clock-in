// Prevents an additional console window on Windows in release builds. Do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The installed app runs under its restart watcher (src/watcher.rs).
    #[cfg(windows)]
    if clockin_lib::watcher::wanted() {
        clockin_lib::watcher::run();
    }
    clockin_lib::run();
}

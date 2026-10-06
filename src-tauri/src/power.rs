//! Windows: keeps the PC from idle-sleeping while Clock In runs (SPEC §8.1,
//! ARCHITECTURE §9). Manual sleep and shutdown still work. The screen may
//! still turn off; an alarm turns it back on.

use windows_sys::Win32::System::Power::{
    ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
};

/// Keeps the system awake until the process ends. Call it on the main
/// thread: the request lasts as long as the thread that made it.
#[allow(unsafe_code)]
pub fn keep_awake() {
    // SAFETY: plain flags in, previous state out; no pointers.
    let previous = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
    if previous == 0 {
        eprintln!("clock-in: Windows refused the keep-awake request");
    }
}

/// Turns the screen on (if it was off for being idle) when an alarm starts.
#[allow(unsafe_code)]
pub fn wake_display() {
    // SAFETY: as above. Without ES_CONTINUOUS this only resets the idle
    // timers once; it doesn't change the continuous keep-awake request.
    unsafe {
        SetThreadExecutionState(ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED);
    }
}

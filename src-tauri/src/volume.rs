//! Windows: whether alarms would be heard (SPEC §6 banner). Checks the
//! default output device with Core Audio (`IAudioEndpointVolume`): muted,
//! volume at zero, or no output device at all.

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};

/// Starts COM on the calling thread (once per thread that calls
/// [`sound_off`]).
#[allow(unsafe_code)]
pub fn init_thread() {
    // SAFETY: no reserved pointer; S_FALSE (already started) is fine too.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

/// True if the default output device is muted, at zero volume, or missing.
/// Unknown (a Core Audio error other than "no device") counts as fine, so a
/// glitch never shows a false banner.
#[allow(unsafe_code)]
#[must_use]
pub fn sound_off() -> bool {
    // SAFETY: standard Core Audio calls on a thread where COM is started
    // (`init_thread`); the interfaces are released when dropped.
    unsafe {
        let Ok(enumerator) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return false;
        };
        let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else {
            // No speakers or headphones at all.
            return true;
        };
        let Ok(volume) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else {
            return false;
        };
        let muted = volume.GetMute().is_ok_and(|m| m.as_bool());
        let zero = volume.GetMasterVolumeLevelScalar().is_ok_and(|v| v <= 0.0);
        muted || zero
    }
}

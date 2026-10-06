//! The alarm sounds (SPEC §7.1, ARCHITECTURE §9): the two generated WAVs,
//! looped with `rodio` on Windows. Other desktops (Linux CI, development)
//! play nothing; Android rings from its Kotlin plugin (Phase 5).
//!
//! The player runs on its own thread and opens the default output device
//! each time a sound starts, so a device plugged in or changed since the
//! last alarm is used.

use clockin_core::Sound;

// Decoded only by the Windows player (and the tests).
#[cfg(any(windows, test))]
const CHECK_IN_WAV: &[u8] = include_bytes!("../../assets/sounds/check-in.wav");
#[cfg(any(windows, test))]
const CHECK_OUT_WAV: &[u8] = include_bytes!("../../assets/sounds/check-out.wav");

/// Decoded PCM audio.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub channels: u16,
    pub sample_rate: u32,
    /// Interleaved samples in −1.0 … 1.0.
    pub samples: Vec<f32>,
}

/// Reads a 16-bit PCM WAV (the format `tools/gen-sounds` writes).
#[cfg(any(windows, test))]
#[must_use]
pub fn parse_wav(bytes: &[u8]) -> Option<Pcm> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let u16_at = |i: usize| Some(u16::from_le_bytes(bytes.get(i..i + 2)?.try_into().ok()?));
    let u32_at = |i: usize| Some(u32::from_le_bytes(bytes.get(i..i + 4)?.try_into().ok()?));
    let mut format = None;
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = usize::try_from(u32_at(pos + 4)?).ok()?;
        let body = pos + 8;
        let end = body.checked_add(len)?;
        if id == b"fmt " {
            let (tag, channels, rate, bits) = (
                u16_at(body)?,
                u16_at(body + 2)?,
                u32_at(body + 4)?,
                u16_at(body + 14)?,
            );
            if tag != 1 || bits != 16 || channels == 0 || rate == 0 {
                return None;
            }
            format = Some((channels, rate));
        } else if id == b"data" {
            let (channels, sample_rate) = format?;
            let data = bytes.get(body..end.min(bytes.len()))?;
            let samples = data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&b| f32::from(i16::from_le_bytes(b)) / 32768.0)
                .collect();
            return Some(Pcm {
                channels,
                sample_rate,
                samples,
            });
        }
        // Chunks are padded to an even length.
        pos = end + (len & 1);
    }
    None
}

/// The decoded sound for an alarm event.
#[cfg(any(windows, test))]
#[must_use]
pub fn pcm(sound: Sound) -> Option<Pcm> {
    parse_wav(match sound {
        Sound::CheckIn => CHECK_IN_WAV,
        Sound::CheckOut => CHECK_OUT_WAV,
    })
}

#[cfg(windows)]
pub use windows::Player;

#[cfg(windows)]
mod windows {
    use super::pcm;
    use clockin_core::Sound;
    use rodio::Source;
    use std::num::NonZero;
    use std::sync::mpsc;

    /// Plays one alarm sound in a loop, or nothing.
    pub struct Player {
        tx: mpsc::Sender<Option<Sound>>,
    }

    impl Player {
        #[must_use]
        pub fn start() -> Self {
            let (tx, rx) = mpsc::channel();
            std::thread::Builder::new()
                .name("clock-in-audio".into())
                .spawn(move || run(&rx))
                .ok();
            Self { tx }
        }

        /// Loops `sound` (from the start if it wasn't already playing), or
        /// stops with `None`.
        pub fn set(&self, sound: Option<Sound>) {
            let _ = self.tx.send(sound);
        }
    }

    struct Playing {
        sound: Sound,
        _sink: rodio::MixerDeviceSink,
        _player: rodio::Player,
    }

    fn run(rx: &mpsc::Receiver<Option<Sound>>) {
        let mut playing: Option<Playing> = None;
        for wanted in rx {
            if playing.as_ref().map(|p| p.sound) == wanted {
                continue;
            }
            // Dropping the sink stops the sound.
            playing = None;
            if let Some(sound) = wanted {
                match open(sound) {
                    Ok(p) => playing = Some(p),
                    // Retried at the scheduler's next evaluation.
                    Err(e) => eprintln!("clock-in: cannot play the alarm sound: {e}"),
                }
            }
        }
    }

    fn open(sound: Sound) -> Result<Playing, String> {
        let pcm = pcm(sound).ok_or("bad sound file")?;
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().map_err(|e| e.to_string())?;
        sink.log_on_drop(false);
        let player = rodio::Player::connect_new(sink.mixer());
        let channels = NonZero::new(pcm.channels).ok_or("no channels")?;
        let rate = NonZero::new(pcm.sample_rate).ok_or("no sample rate")?;
        player.append(
            rodio::buffer::SamplesBuffer::new(channels, rate, pcm.samples).repeat_infinite(),
        );
        Ok(Playing {
            sound,
            _sink: sink,
            _player: player,
        })
    }
}

/// Plays nothing (desktops other than Windows: development only).
#[cfg(not(windows))]
pub struct Player;

#[cfg(not(windows))]
impl Player {
    #[must_use]
    pub fn start() -> Self {
        Self
    }

    pub fn set(&self, _sound: Option<Sound>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sounds_decode() {
        for sound in [Sound::CheckIn, Sound::CheckOut] {
            let pcm = pcm(sound).unwrap();
            assert_eq!(pcm.channels, 1);
            assert_eq!(pcm.sample_rate, 44_100);
            // About 2 s (ARCHITECTURE §11).
            let secs = pcm.samples.len() as f64 / 44_100.0;
            assert!((1.9..=2.1).contains(&secs), "{secs}");
            assert!(pcm.samples.iter().all(|s| (-1.0..=1.0).contains(s)));
            assert!(pcm.samples.iter().any(|s| s.abs() > 0.5), "not silent");
        }
    }

    #[test]
    fn rejects_other_files() {
        assert_eq!(parse_wav(b""), None);
        assert_eq!(parse_wav(b"RIFF\0\0\0\0WAVE"), None);
        assert_eq!(parse_wav(&CHECK_IN_WAV[..40]), None, "truncated");
        let mut float = CHECK_IN_WAV.to_vec();
        float[20] = 3; // format tag 3 = IEEE float
        assert_eq!(parse_wav(&float), None);
    }
}

//! Synthesises the two alarm sounds into `assets/sounds/` (ARCHITECTURE §11):
//!
//! - `check-in.wav`: a bright, rising three-note major arpeggio (C5–E5–G5)
//!   with a soft bell timbre;
//! - `check-out.wav`: a softer, falling two-note figure (G5–C5) with a rounder
//!   timbre and a gentler attack.
//!
//! Both are 44.1 kHz, 16-bit mono, exactly 2 s long, and start and end on
//! silence so they loop seamlessly. The output is fully deterministic (no
//! randomness), so re-running the tool reproduces the committed files.
//!
//! Usage: `cargo run -p gen-sounds [-- <output dir>]`

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

const SAMPLE_RATE: u32 = 44_100;
const LOOP_SECONDS: f64 = 2.0;
const SAMPLES: usize = 88_200; // SAMPLE_RATE × LOOP_SECONDS

/// One partial of a bell-like tone: frequency ratio to the fundamental,
/// relative amplitude, and decay time constant in seconds.
struct Partial {
    ratio: f64,
    amp: f64,
    decay_s: f64,
}

/// A note: fundamental frequency, onset time and loudness.
struct Note {
    freq: f64,
    onset_s: f64,
    gain: f64,
}

/// A voice: its partials, attack time and how long it rings.
struct Voice {
    partials: &'static [Partial],
    attack_s: f64,
}

/// Bright, soft bell: strong low partials, a slightly inharmonic shimmer that
/// dies away quickly.
const BELL: Voice = Voice {
    partials: &[
        Partial {
            ratio: 1.0,
            amp: 1.0,
            decay_s: 0.55,
        },
        Partial {
            ratio: 2.0,
            amp: 0.45,
            decay_s: 0.35,
        },
        Partial {
            ratio: 3.0,
            amp: 0.18,
            decay_s: 0.22,
        },
        Partial {
            ratio: 4.16,
            amp: 0.10,
            decay_s: 0.12,
        },
        Partial {
            ratio: 5.43,
            amp: 0.05,
            decay_s: 0.08,
        },
    ],
    attack_s: 0.006,
};

/// Rounder, softer voice for check-out: mostly fundamental, slower attack.
const SOFT: Voice = Voice {
    partials: &[
        Partial {
            ratio: 1.0,
            amp: 1.0,
            decay_s: 0.60,
        },
        Partial {
            ratio: 2.0,
            amp: 0.20,
            decay_s: 0.30,
        },
        Partial {
            ratio: 3.0,
            amp: 0.05,
            decay_s: 0.15,
        },
    ],
    attack_s: 0.025,
};

const C5: f64 = 523.251;
const E5: f64 = 659.255;
const G5: f64 = 783.991;

/// Final fade so the loop point is exactly silent.
const RELEASE_S: f64 = 0.25;

/// Peak level of each sound (full scale = 1.0).
const CHECK_IN_PEAK: f64 = 0.89; // ≈ −1 dBFS
const CHECK_OUT_PEAK: f64 = 0.63; // ≈ −4 dBFS, softer

fn check_in() -> Vec<i16> {
    let notes = [
        Note {
            freq: C5,
            onset_s: 0.00,
            gain: 0.85,
        },
        Note {
            freq: E5,
            onset_s: 0.16,
            gain: 0.90,
        },
        Note {
            freq: G5,
            onset_s: 0.32,
            gain: 1.00,
        },
    ];
    render(&BELL, &notes, CHECK_IN_PEAK)
}

fn check_out() -> Vec<i16> {
    let notes = [
        Note {
            freq: G5,
            onset_s: 0.00,
            gain: 0.90,
        },
        Note {
            freq: C5,
            onset_s: 0.32,
            gain: 1.00,
        },
    ];
    render(&SOFT, &notes, CHECK_OUT_PEAK)
}

fn render(voice: &Voice, notes: &[Note], peak: f64) -> Vec<i16> {
    let rate = f64::from(SAMPLE_RATE);
    let mut buf = vec![0.0_f64; SAMPLES];
    for note in notes {
        for (i, sample) in buf.iter_mut().enumerate() {
            let t = i as f64 / rate - note.onset_s;
            if t < 0.0 {
                continue;
            }
            // Raised-cosine attack avoids a click at each onset.
            let attack = if t < voice.attack_s {
                0.5 - 0.5 * (PI * t / voice.attack_s).cos()
            } else {
                1.0
            };
            let tone: f64 = voice
                .partials
                .iter()
                .map(|p| {
                    p.amp * (-t / p.decay_s).exp() * (2.0 * PI * note.freq * p.ratio * t).sin()
                })
                .sum();
            *sample += note.gain * attack * tone;
        }
    }
    // Release: cosine fade to exactly zero at the loop point.
    let release_start = LOOP_SECONDS - RELEASE_S;
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / rate;
        if t >= release_start {
            let x = ((t - release_start) / RELEASE_S).min(1.0);
            *sample *= 0.5 + 0.5 * (PI * x).cos();
        }
    }
    buf[SAMPLES - 1] = 0.0;
    let max = buf.iter().fold(0.0_f64, |m, s| m.max(s.abs()));
    let scale = if max > 0.0 { peak / max } else { 0.0 };
    buf.iter()
        .map(|s| {
            let v = (s * scale * f64::from(i16::MAX)).round();
            // `peak` ≤ 1, so `v` is always inside the i16 range.
            v.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
        })
        .collect()
}

/// A canonical 44-byte-header PCM WAV file.
fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len() * 2).expect("sound fits in a WAV file");
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16_u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1_u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2_u16.to_le_bytes()); // block align
    out.extend_from_slice(&16_u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn default_out_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds")
}

fn main() -> std::io::Result<()> {
    let out_dir = std::env::args_os()
        .nth(1)
        .map_or_else(default_out_dir, PathBuf::from);
    std::fs::create_dir_all(&out_dir)?;
    for (name, samples) in [("check-in.wav", check_in()), ("check-out.wav", check_out())] {
        let path = out_dir.join(name);
        std::fs::write(&path, wav_bytes(&samples))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Energy at `freq` in `samples[from..to]` (Goertzel algorithm).
    fn energy(samples: &[i16], freq: f64, from_s: f64, to_s: f64) -> f64 {
        let rate = f64::from(SAMPLE_RATE);
        let (from, to) = ((from_s * rate) as usize, (to_s * rate) as usize);
        let coeff = 2.0 * (2.0 * PI * freq / rate).cos();
        let (mut s1, mut s2) = (0.0, 0.0);
        for &x in &samples[from..to] {
            let s0 = f64::from(x) + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        s1 * s1 + s2 * s2 - coeff * s1 * s2
    }

    fn rms(samples: &[i16]) -> f64 {
        let sum: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
        (sum / samples.len() as f64).sqrt()
    }

    fn peak(samples: &[i16]) -> i32 {
        samples.iter().map(|&s| i32::from(s).abs()).max().unwrap()
    }

    #[test]
    fn both_sounds_are_two_seconds_and_loop_on_silence() {
        for samples in [check_in(), check_out()] {
            assert_eq!(samples.len(), SAMPLES);
            assert_eq!(samples[0], 0, "starts on silence");
            assert_eq!(samples[SAMPLES - 1], 0, "ends on silence");
            // The last 10 ms are inaudible, so the wrap-around never clicks.
            assert!(samples[SAMPLES - 441..].iter().all(|s| s.abs() < 50));
        }
    }

    #[test]
    fn levels_check_in_louder_than_check_out() {
        let (cin, cout) = (check_in(), check_out());
        let full = f64::from(i16::MAX);
        assert!((f64::from(peak(&cin)) / full - CHECK_IN_PEAK).abs() < 0.001);
        assert!((f64::from(peak(&cout)) / full - CHECK_OUT_PEAK).abs() < 0.001);
        assert!(rms(&cout) < rms(&cin));
    }

    #[test]
    fn check_in_rises() {
        let s = check_in();
        // C5 dominates the first note, G5 the last.
        assert!(energy(&s, C5, 0.02, 0.15) > 10.0 * energy(&s, G5, 0.02, 0.15));
        assert!(energy(&s, G5, 0.40, 0.70) > energy(&s, C5, 0.40, 0.70));
        assert!(energy(&s, E5, 0.18, 0.31) > energy(&s, C5, 0.18, 0.31));
    }

    #[test]
    fn check_out_falls() {
        let s = check_out();
        assert!(energy(&s, G5, 0.02, 0.30) > 10.0 * energy(&s, C5, 0.02, 0.30));
        assert!(energy(&s, C5, 0.40, 0.80) > energy(&s, G5, 0.40, 0.80));
    }

    #[test]
    fn wav_header_is_canonical_pcm() {
        let bytes = wav_bytes(&check_in());
        assert_eq!(bytes.len(), 44 + SAMPLES * 2);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize,
            bytes.len() - 8
        );
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1, "PCM");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1, "mono");
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            44_100
        );
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16, "16-bit");
        assert_eq!(&bytes[36..40], b"data");
    }

    /// The committed files are what the generator produces. Floating-point
    /// `sin`/`exp` may differ by a rounding step between platforms, so allow
    /// ±1 per sample.
    #[test]
    fn committed_files_match_the_generator() {
        let dir = default_out_dir();
        for (name, samples) in [("check-in.wav", check_in()), ("check-out.wav", check_out())] {
            let committed = std::fs::read(dir.join(name))
                .unwrap_or_else(|e| panic!("{name}: {e}; run `cargo run -p gen-sounds`"));
            let expected = wav_bytes(&samples);
            assert_eq!(committed.len(), expected.len(), "{name}");
            assert_eq!(committed[..44], expected[..44], "{name} header");
            for (i, (c, e)) in committed[44..]
                .chunks_exact(2)
                .zip(expected[44..].chunks_exact(2))
                .enumerate()
            {
                let c = i16::from_le_bytes([c[0], c[1]]);
                let e = i16::from_le_bytes([e[0], e[1]]);
                assert!(
                    (i32::from(c) - i32::from(e)).abs() <= 1,
                    "{name} sample {i}: {c} vs {e}"
                );
            }
        }
    }
}

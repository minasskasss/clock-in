//! The ring-mode repeat cycle (SPEC §7.3): ring 5 minutes, silent 5 minutes,
//! repeat until Stop.

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

/// How long each ring lasts.
pub const RING_DURATION: SignedDuration = SignedDuration::from_mins(5);
/// How long the silence between rings lasts.
pub const SILENT_DURATION: SignedDuration = SignedDuration::from_mins(5);

/// Where an unstopped alarm is in its repeat cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum RingPhase {
    /// Sound playing. `cycle` counts rings from 0; the sound stops at `silent_at`.
    Ringing { cycle: u32, silent_at: Timestamp },
    /// Sound paused. The next ring (`cycle + 1`) starts at `rering_at`; before
    /// it, the caller drops names already marked
    /// ([`crate::AlarmEvent::still_due`]).
    Silent { cycle: u32, rering_at: Timestamp },
}

/// The phase at `now` of a cycle that started ringing at `started_at`.
///
/// If the clock went backwards (`now` before `started_at`), it counts as the
/// first ring, so an alarm is never silenced by a clock change.
#[must_use]
pub fn ring_cycle_state(started_at: Timestamp, now: Timestamp) -> RingPhase {
    let period = (RING_DURATION + SILENT_DURATION).as_secs();
    let elapsed = now.duration_since(started_at).as_secs().max(0);
    let cycle = elapsed / period;
    let into_cycle = elapsed % period;
    let cycle_start = started_at
        .saturating_add(SignedDuration::from_secs(cycle * period))
        .unwrap_or(started_at);
    let cycle_number = u32::try_from(cycle).unwrap_or(u32::MAX);
    if into_cycle < RING_DURATION.as_secs() {
        RingPhase::Ringing {
            cycle: cycle_number,
            silent_at: cycle_start
                .saturating_add(RING_DURATION)
                .unwrap_or(cycle_start),
        }
    } else {
        RingPhase::Silent {
            cycle: cycle_number,
            rering_at: cycle_start
                .saturating_add(RING_DURATION + SILENT_DURATION)
                .unwrap_or(cycle_start),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(min: i64, sec: i64) -> Timestamp {
        let start: Timestamp = "2026-06-01T06:00:00Z".parse().unwrap();
        start + SignedDuration::from_secs(min * 60 + sec)
    }

    #[test]
    fn ring_then_silence_then_ring_again() {
        let start = at(0, 0);
        let cases = [
            (
                at(0, 0),
                RingPhase::Ringing {
                    cycle: 0,
                    silent_at: at(5, 0),
                },
            ),
            (
                at(4, 59),
                RingPhase::Ringing {
                    cycle: 0,
                    silent_at: at(5, 0),
                },
            ),
            (
                at(5, 0),
                RingPhase::Silent {
                    cycle: 0,
                    rering_at: at(10, 0),
                },
            ),
            (
                at(9, 59),
                RingPhase::Silent {
                    cycle: 0,
                    rering_at: at(10, 0),
                },
            ),
            (
                at(10, 0),
                RingPhase::Ringing {
                    cycle: 1,
                    silent_at: at(15, 0),
                },
            ),
            (
                at(14, 59),
                RingPhase::Ringing {
                    cycle: 1,
                    silent_at: at(15, 0),
                },
            ),
            (
                at(15, 0),
                RingPhase::Silent {
                    cycle: 1,
                    rering_at: at(20, 0),
                },
            ),
            (
                at(20, 0),
                RingPhase::Ringing {
                    cycle: 2,
                    silent_at: at(25, 0),
                },
            ),
            (
                at(605, 0),
                RingPhase::Silent {
                    cycle: 60,
                    rering_at: at(610, 0),
                },
            ),
        ];
        for (now, expected) in cases {
            assert_eq!(ring_cycle_state(start, now), expected, "{now}");
        }
    }

    #[test]
    fn clock_going_backwards_keeps_ringing() {
        assert_eq!(
            ring_cycle_state(at(0, 0), at(-3, 0)),
            RingPhase::Ringing {
                cycle: 0,
                silent_at: at(5, 0)
            }
        );
    }

    #[test]
    fn serialises_with_a_phase_tag() {
        let json = serde_json::to_value(ring_cycle_state(at(0, 0), at(6, 0))).unwrap();
        assert_eq!(json["phase"], "silent");
        assert_eq!(json["cycle"], 0);
    }
}

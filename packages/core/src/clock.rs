//! Time, as a port.
//!
//! WHY: "clock skew" is on the list of device faults the simulator must be able
//! to produce, and a device that has been asleep for a week comes back with a
//! clock that may have jumped, gone backwards, or not moved at all. Code that
//! calls `Utc::now()` directly cannot be tested against any of that.
//!
//! The safety-critical paths were already written to take `now` as a parameter
//! — [`crate::intent::ExplicitUserIntent::is_fresh`] and grant expiry both do.
//! This port is for everything else: the timestamps written into rows, the
//! journal, and eventually the sync watermarks.

use crate::Timestamp;

/// A source of the current time.
///
/// `Send + Sync` so a single clock can be shared by a background sync worker
/// and a foreground request without ceremony.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// The real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        chrono::Utc::now()
    }
}

/// A `'static` instance, so `&SYSTEM_CLOCK` can be the default in a struct that
/// borrows its clock.
pub static SYSTEM_CLOCK: SystemClock = SystemClock;

/// A clock frozen at one instant.
///
/// Deliberately in the library rather than in a test module: the simulator and
/// the device tests both need it, and they live in different crates.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(Timestamp);

impl FixedClock {
    pub fn at(instant: Timestamp) -> Self {
        Self(instant)
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

/// A clock that advances by a fixed step on every read.
///
/// Useful for asserting ordering without sleeping, and for reproducing the
/// case where two writes land in the same millisecond.
#[derive(Debug)]
pub struct SteppingClock {
    start: Timestamp,
    step: chrono::Duration,
    reads: std::sync::atomic::AtomicI64,
}

impl SteppingClock {
    pub fn new(start: Timestamp, step: chrono::Duration) -> Self {
        Self {
            start,
            step,
            reads: std::sync::atomic::AtomicI64::new(0),
        }
    }
}

impl Clock for SteppingClock {
    fn now(&self) -> Timestamp {
        let n = self
            .reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.start + self.step * (n as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn instant(secs: i64) -> Timestamp {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    #[test]
    fn a_fixed_clock_does_not_move() {
        let clock = FixedClock::at(instant(1_000));
        assert_eq!(clock.now(), clock.now());
        assert_eq!(clock.now(), instant(1_000));
    }

    #[test]
    fn a_stepping_clock_advances_predictably() {
        let clock = SteppingClock::new(instant(0), Duration::seconds(10));
        assert_eq!(clock.now(), instant(0));
        assert_eq!(clock.now(), instant(10));
        assert_eq!(clock.now(), instant(20));
    }

    #[test]
    fn the_system_clock_moves_forward() {
        let clock = SystemClock;
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
    }

    #[test]
    fn a_clock_can_be_used_behind_a_trait_object() {
        // The whole point: production passes SystemClock, the simulator passes
        // something that can jump backwards.
        let clocks: Vec<Box<dyn Clock>> =
            vec![Box::new(SystemClock), Box::new(FixedClock::at(instant(42)))];
        assert_eq!(clocks[1].now(), instant(42));
    }
}

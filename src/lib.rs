//! # Ternary Conserve
//!
//! Parametric conservation across resource domains.
//!
//! **When to use this:** you have a finite, depletable budget (fuel, battery
//! charge, fish-stock biomass, LLM token quota, crew attention-hours) and you
//! want to *tick* consumption against it while automatically emitting events
//! whenever a threshold is crossed or the budget runs out. It gives you one
//! generic, `no_std`-friendly abstraction that turns "how much is left?" into
//! actionable [`Ternary`] severity signals — instead of hand-rolling a bespoke
//! budget+alarm struct per resource type.
//!
//! The conservation thesis is central to the Ternary philosophy: measurable resources
//! should be budgeted, profiled, detected, and reported in a closed-loop cycle.
//!
//! ## Domains
//!
//! - **Fish stocks** — catch limits, biomass thresholds
//! - **Fuel** — range management, trip planning
//! - **Battery** — mAh budgeting, charge cycles
//! - **Inference tokens** — LLM call budgets
//! - **Crew attention** — human-hours, meeting costs
//!
//! ## Cycle: Budget → Profile → Detect → Report
//!
//! Every domain follows this cycle:
//!
//! 1. **Budget** — what's allocated (`Budget<T>`)
//! 2. **Profile** — expected consumption patterns (`Profile<T>`)
//! 3. **Detect** — tick consumption, compare against thresholds
//! 4. **Report** — emit `ConservationEvent` when boundaries are breached
//!
//! # Example
//!
//! ```
//! use std::time::Duration;
//! use std::collections::VecDeque;
//! use ternary_conserve::{
//!     ConservationDomain, ResourceUnit, Budget, Profile,
//!     ConservationEvent, EventKind, ThresholdSet,
//! };
//!
//! // 1. BUDGET: Allocate fuel for a 200km trip
//! // 2. PROFILE: Expect 8 L/100km, peaks at 12
//! // 3. DETECT: Tick each segment
//! // 4. REPORT: Threshold crossings become events
//!
//! fn fuel_example() {
//!     let mut fuel = ConservationDomain::new(
//!         "fuel",
//!         Budget { total: 16.0_f64, allocated: 16.0, consumed: 0.0 },
//!         Profile { expected_rate: 0.8, peak: 1.2, variance: 0.15 },
//!         ThresholdSet { warning: 4.0, critical: 2.0, floor: 0.0 },
//!     );
//!
//!     // Tick with normal consumption
//!     assert!(fuel.tick(0.8).is_none()); // within profile
//!
//!     // Tick with a heavy segment (uphill)
//!     let event = fuel.tick(1.2);
//!     assert!(event.is_none()); // peak is the max, not a threshold
//!
//!     // Deplete most fuel — should trigger warning
//!     for _ in 0..10 { let _ = fuel.tick(1.0); }
//!
//!     assert!(fuel.remaining() <= 4.0, "should be at or below warning");
//! }
//!
//! fuel_example();
//! ```

#![cfg_attr(not(test), no_std)]
#![deny(missing_docs)]
#![deny(unsafe_code)]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use core::fmt::Debug;
use core::time::Duration;

/// Re-export of [`ternary_types::Ternary`] — the three-valued severity used by
/// [`ConservationEvent`]. Re-exported here so callers can match on an event's
/// severity without adding `ternary-types` as a separate dependency.
pub use ternary_types::Ternary;

/// A measurable resource unit.
///
/// Implementations for common numeric types are provided.
///
/// ```
/// use ternary_conserve::ResourceUnit;
///
/// fn check<T: ResourceUnit>(val: T) -> Option<T> {
///     assert_eq!(T::zero().remaining(&val), Some(val.partial_max()?));
///     Some(val)
/// }
/// ```
pub trait ResourceUnit: Copy + Clone + Debug + PartialOrd + PartialEq {
    /// The zero/empty value of this resource.
    fn zero() -> Self;

    /// Compute remaining after consuming `consumed` units.
    ///
    /// Returns `None` when consumption exceeds the total (overdraft).
    /// Otherwise returns `Some(remaining)`.
    fn remaining(&self, consumed: &Self) -> Option<Self>;

    /// An estimate of "max" for projection purposes.
    /// Used internally for rate calculation and projection.
    #[doc(hidden)]
    fn partial_max(&self) -> Option<Self>;

    /// Convert to f64 for arithmetic.
    #[doc(hidden)]
    fn to_f64(&self) -> f64;

    /// Construct from f64 (clamping/closest representation).
    #[doc(hidden)]
    fn from_f64(v: f64) -> Self;
}

// ---------------------------------------------------------------------------
// ResourceUnit blanket impls for primitives
// ---------------------------------------------------------------------------

macro_rules! impl_resource_unit_float {
    ($ty:ty) => {
        impl ResourceUnit for $ty {
            #[inline]
            fn zero() -> Self {
                0.0
            }

            #[inline]
            fn remaining(&self, consumed: &Self) -> Option<Self> {
                let r = self - consumed;
                if r < 0.0 {
                    None
                } else {
                    Some(r)
                }
            }

            #[inline]
            fn partial_max(&self) -> Option<Self> {
                Some(*self)
            }

            #[inline]
            fn to_f64(&self) -> f64 {
                *self as f64
            }

            #[inline]
            fn from_f64(v: f64) -> Self {
                v as Self
            }
        }
    };
}

impl_resource_unit_float!(f32);
impl_resource_unit_float!(f64);

macro_rules! impl_resource_unit_int {
    ($ty:ty) => {
        impl ResourceUnit for $ty {
            #[inline]
            fn zero() -> Self {
                0
            }

            #[inline]
            fn remaining(&self, consumed: &Self) -> Option<Self> {
                let r = self.checked_sub(*consumed)?;
                Some(r)
            }

            #[inline]
            fn partial_max(&self) -> Option<Self> {
                Some(*self)
            }

            #[inline]
            fn to_f64(&self) -> f64 {
                *self as f64
            }

            #[inline]
            fn from_f64(v: f64) -> Self {
                v as Self
            }
        }
    };
}

impl_resource_unit_int!(u8);
impl_resource_unit_int!(u16);
impl_resource_unit_int!(u32);
impl_resource_unit_int!(u64);
impl_resource_unit_int!(u128);
impl_resource_unit_int!(usize);
impl_resource_unit_int!(i8);
impl_resource_unit_int!(i16);
impl_resource_unit_int!(i32);
impl_resource_unit_int!(i64);
impl_resource_unit_int!(i128);
impl_resource_unit_int!(isize);

/// Budget: what's allocated for a conservation domain.
///
/// ```
/// use ternary_conserve::Budget;
///
/// let b = Budget { total: 100_u32, allocated: 80, consumed: 0 };
/// assert_eq!(b.total, 100);
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Budget<T> {
    /// Total resource available (the grant/allocation).
    pub total: T,
    /// How much was actually allocated/earmarked.
    pub allocated: T,
    /// How much has been consumed so far.
    pub consumed: T,
}

/// Profile: expected consumption patterns for a domain.
///
/// ```
/// use ternary_conserve::Profile;
///
/// let p = Profile { expected_rate: 0.8_f64, peak: 1.2, variance: 0.15 };
/// assert!(p.peak >= p.expected_rate);
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Profile<T> {
    /// Expected consumption per tick.
    pub expected_rate: T,
    /// Maximum consumption per tick (the peak).
    pub peak: T,
    /// Expected deviation (coefficient of variation-style).
    pub variance: f64,
}

/// The kind of conservation event that was triggered.
#[derive(Clone, Debug, PartialEq)]
pub enum EventKind<T> {
    /// Budget was exceeded — actual consumption went over the limit.
    BudgetExceeded {
        /// What was actually consumed.
        actual: T,
        /// What the limit was.
        limit: T,
    },
    /// A rate anomaly — actual consumption deviated from expected.
    RateAnomaly {
        /// What was expected.
        expected: T,
        /// What was actually consumed.
        actual: T,
    },
    /// A threshold was crossed.
    ThresholdCrossed {
        /// Human-readable name of the threshold.
        threshold_name: &'static str,
        /// The current value when crossed.
        value: T,
    },
    /// A cascade — one domain's event triggered action in another.
    Cascade {
        /// Description of what triggered the cascade.
        trigger: String,
        /// Which domain was affected.
        affected_domain: &'static str,
    },
}

/// A conservation event — something worth reporting.
///
/// Severity uses `ternary_types::Ternary`:
/// - `Negative` → bad (budget exceeded, critical threshold)
/// - `Neutral` → warning (rate anomaly, warning threshold)
/// - `Positive` → healthy (everything nominal)
#[derive(Clone, Debug, PartialEq)]
pub struct ConservationEvent<T> {
    /// When the event occurred.
    pub timestamp: Duration,
    /// Which domain triggered this event.
    pub domain: &'static str,
    /// What kind of event.
    pub kind: EventKind<T>,
    /// Severity: Negative=bad, Neutral=warn, Positive=healthy.
    pub severity: Ternary,
}

/// ThresholdSet: when to alert.
///
/// ```
/// use ternary_conserve::ThresholdSet;
///
/// let ts = ThresholdSet { warning: 4.0_f64, critical: 2.0, floor: 0.0 };
/// assert!(ts.warning > ts.critical);
/// assert!(ts.critical > ts.floor);
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ThresholdSet<T> {
    /// Warning threshold — below this triggers an event with `Neutral` severity.
    pub warning: T,
    /// Critical threshold — below this triggers an event with `Negative` severity (urgent).
    pub critical: T,
    /// Hard floor — the absolute minimum before exhaustion.
    pub floor: T,
}

/// A parametric conservation domain.
///
/// Every domain follows the **Budget → Profile → Detect → Report** cycle.
///
/// # Type Parameters
///
/// - `T`: The resource unit type (must implement [`ResourceUnit`]).
///
/// # Example
///
/// ```
/// use std::collections::VecDeque;
/// use ternary_conserve::{ConservationDomain, Budget, Profile, ThresholdSet};
///
/// let mut battery = ConservationDomain::new(
///     "battery_mAh",
///     Budget { total: 5000_f64, allocated: 4500.0, consumed: 0.0 },
///     Profile { expected_rate: 120.0, peak: 300.0, variance: 0.1 },
///     ThresholdSet { warning: 1000.0, critical: 500.0, floor: 100.0 },
/// );
///
/// // Normal consumption — no event
/// assert!(battery.tick(120.0).is_none());
///
/// // Heavy consumption
/// let _ = battery.tick(300.0);
///
/// // Check remaining
/// let rem = battery.remaining();
/// assert!(rem < 5000.0);
/// ```
#[derive(Clone, Debug)]
pub struct ConservationDomain<T: ResourceUnit> {
    /// Human-readable domain name (e.g. "fuel", "battery_mAh", "crew_minutes").
    pub name: &'static str,
    /// The resource budget.
    pub budget: Budget<T>,
    /// Expected consumption profile.
    pub profile: Profile<T>,
    /// Rolling event history (capped at 1024 entries).
    pub history: VecDeque<ConservationEvent<T>>,
    /// Thresholds for alerting.
    pub thresholds: ThresholdSet<T>,
    /// Monotonic tick counter (for internal rate calculation).
    ticks: u64,
}

impl<T: ResourceUnit> ConservationDomain<T> {
    /// Create a new conservation domain.
    ///
    /// Panics if `thresholds` are not ordered: `warning > critical > floor`.
    ///
    /// ```
    /// use ternary_conserve::*;
    ///
    /// let d = ConservationDomain::new(
    ///     "test",
    ///     Budget { total: 100_u32, allocated: 80, consumed: 0 },
    ///     Profile { expected_rate: 10_u32, peak: 25, variance: 0.2 },
    ///     ThresholdSet { warning: 30, critical: 15, floor: 5 },
    /// );
    /// assert_eq!(d.name, "test");
    /// assert_eq!(d.remaining(), 100);
    /// ```
    pub fn new(
        name: &'static str,
        budget: Budget<T>,
        profile: Profile<T>,
        thresholds: ThresholdSet<T>,
    ) -> Self {
        // Validate threshold ordering: warning >= critical >= floor.
        // This is a hard `assert!` (not `debug_assert!`) because the doc
        // contract promises a panic on invalid ordering, and degenerate
        // threshold configurations silently produce wrong/missing events
        // in release builds if left unchecked.
        assert!(
            thresholds.warning >= thresholds.critical && thresholds.critical >= thresholds.floor,
            "thresholds must be ordered: warning >= critical >= floor"
        );

        Self {
            name,
            budget,
            profile,
            history: VecDeque::with_capacity(128),
            thresholds,
            ticks: 0,
        }
    }

    /// Tick the domain: record consumption and check thresholds.
    ///
    /// Returns `Some(event)` if a threshold was crossed or an anomaly detected.
    /// Returns `None` for nominal consumption within profile.
    ///
    /// ```
    /// use ternary_conserve::*;
    ///
    /// let mut fuel = ConservationDomain::new(
    ///     "fuel",
    ///     Budget { total: 100.0, allocated: 100.0, consumed: 0.0 },
    ///     Profile { expected_rate: 10.0, peak: 20.0, variance: 0.1 },
    ///     ThresholdSet { warning: 20.0, critical: 10.0, floor: 5.0 },
    /// );
    ///
    /// // Nominal tick
    /// assert!(fuel.tick(10.0).is_none());
    ///
    /// // Consume more
    /// let _ = fuel.tick(10.0);
    /// ```
    pub fn tick(&mut self, consumed: T) -> Option<ConservationEvent<T>> {
        self.ticks += 1;
        let timestamp = Duration::from_secs(self.ticks);

        // 1. Update budget consumption
        self.budget.consumed = T::from_f64(self.budget.consumed.to_f64() + consumed.to_f64());

        // 2. Calculate remaining
        let remaining = match self.budget.total.remaining(&self.budget.consumed) {
            Some(r) => r,
            None => {
                // Budget exceeded
                let event = ConservationEvent {
                    timestamp,
                    domain: self.name,
                    kind: EventKind::BudgetExceeded {
                        actual: self.budget.consumed,
                        limit: self.budget.total,
                    },
                    severity: Ternary::Negative,
                };
                self.history.push_back(event.clone());
                self.trim_history();
                return Some(event);
            }
        };

        // 3. Check thresholds (floor > critical > warning)
        if remaining <= self.thresholds.floor {
            // Hard stop — critical cascade
            let event = ConservationEvent {
                timestamp,
                domain: self.name,
                kind: EventKind::ThresholdCrossed {
                    threshold_name: "floor",
                    value: remaining,
                },
                severity: Ternary::Negative,
            };
            self.history.push_back(event.clone());
            self.trim_history();
            return Some(event);
        }

        if remaining <= self.thresholds.critical {
            let event = ConservationEvent {
                timestamp,
                domain: self.name,
                kind: EventKind::ThresholdCrossed {
                    threshold_name: "critical",
                    value: remaining,
                },
                severity: Ternary::Negative,
            };
            self.history.push_back(event.clone());
            self.trim_history();
            return Some(event);
        }

        if remaining <= self.thresholds.warning {
            let event = ConservationEvent {
                timestamp,
                domain: self.name,
                kind: EventKind::ThresholdCrossed {
                    threshold_name: "warning",
                    value: remaining,
                },
                severity: Ternary::Neutral,
            };
            self.history.push_back(event.clone());
            self.trim_history();
            return Some(event);
        }

        // 4. Nominal — no event
        None
    }

    /// Get the current remaining resource.
    ///
    /// ```
    /// use ternary_conserve::*;
    ///
    /// let mut d = ConservationDomain::new(
    ///     "test", Budget { total: 100_u32, allocated: 80, consumed: 0 },
    ///     Profile { expected_rate: 10, peak: 20, variance: 0.1 },
    ///     ThresholdSet { warning: 30, critical: 15, floor: 5 },
    /// );
    /// assert_eq!(d.remaining(), 100);
    /// let _ = d.tick(20);
    /// assert_eq!(d.remaining(), 80);
    /// ```
    pub fn remaining(&self) -> T {
        let remaining = self.budget.total.remaining(&self.budget.consumed);
        remaining.unwrap_or(T::zero())
    }

    /// Calculate the current consumption rate per tick.
    ///
    /// Returns 0.0 if there have been no ticks yet.
    ///
    /// ```
    /// use ternary_conserve::*;
    ///
    /// let mut d = ConservationDomain::new(
    ///     "test", Budget { total: 100_f64, allocated: 100.0, consumed: 0.0 },
    ///     Profile { expected_rate: 10.0, peak: 20.0, variance: 0.1 },
    ///     ThresholdSet { warning: 30.0, critical: 15.0, floor: 5.0 },
    /// );
    /// assert_eq!(d.rate(), 0.0); // no ticks yet
    /// let _ = d.tick(20.0);
    /// let _ = d.tick(10.0);
    /// assert!((d.rate() - 15.0).abs() < 0.01);
    /// ```
    pub fn rate(&self) -> f64 {
        if self.ticks == 0 {
            return 0.0;
        }
        self.budget.consumed.to_f64() / self.ticks as f64
    }

    /// Project time until depletion at the current rate.
    ///
    /// Returns `Duration::MAX` if the rate is zero (no consumption).
    ///
    /// ```
    /// use ternary_conserve::*;
    /// use std::time::Duration;
    ///
    /// let mut d = ConservationDomain::new(
    ///     "test", Budget { total: 100_f64, allocated: 100.0, consumed: 0.0 },
    ///     Profile { expected_rate: 10.0, peak: 20.0, variance: 0.1 },
    ///     ThresholdSet { warning: 30.0, critical: 15.0, floor: 5.0 },
    /// );
    /// assert_eq!(d.project_remaining(), Duration::MAX); // no consumption yet
    /// let _ = d.tick(10.0);
    /// let _ = d.tick(10.0);
    /// let proj = d.project_remaining();
    /// assert!(proj.as_secs() > 0);
    /// ```
    pub fn project_remaining(&self) -> Duration {
        let rate = self.rate();
        if rate <= 0.0 {
            return Duration::MAX;
        }
        let remaining = self.remaining().to_f64();
        if remaining <= 0.0 {
            return Duration::ZERO;
        }
        let ticks_remaining = (remaining / rate).ceil() as u64;
        Duration::from_secs(ticks_remaining)
    }

    /// Get the total number of ticks processed.
    pub fn tick_count(&self) -> u64 {
        self.ticks
    }

    /// Clear event history.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Trim history if it exceeds the max capacity.
    fn trim_history(&mut self) {
        const MAX_HISTORY: usize = 1024;
        while self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }
    }
}

// ---------------------------------------------------------------------------
// Serde support (behind feature flag)
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
mod serde_impl {
    use serde::{Deserialize, Serialize};

    use crate::{Budget, Profile, ResourceUnit, ThresholdSet};

    impl<T: ResourceUnit + Serialize> Serialize for Budget<T> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeStruct;
            let mut s = serializer.serialize_struct("Budget", 3)?;
            s.serialize_field("total", &self.total)?;
            s.serialize_field("allocated", &self.allocated)?;
            s.serialize_field("consumed", &self.consumed)?;
            s.end()
        }
    }

    impl<'de, T: ResourceUnit + Deserialize<'de>> Deserialize<'de> for Budget<T> {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            #[derive(Deserialize)]
            struct BudgetHelper<T> {
                total: T,
                allocated: T,
                consumed: T,
            }
            let h = BudgetHelper::deserialize(deserializer)?;
            Ok(Budget {
                total: h.total,
                allocated: h.allocated,
                consumed: h.consumed,
            })
        }
    }

    impl<T: ResourceUnit + Serialize> Serialize for Profile<T> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeStruct;
            let mut s = serializer.serialize_struct("Profile", 3)?;
            s.serialize_field("expected_rate", &self.expected_rate)?;
            s.serialize_field("peak", &self.peak)?;
            s.serialize_field("variance", &self.variance)?;
            s.end()
        }
    }

    impl<'de, T: ResourceUnit + Deserialize<'de>> Deserialize<'de> for Profile<T> {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            #[derive(Deserialize)]
            struct ProfileHelper<T> {
                expected_rate: T,
                peak: T,
                variance: f64,
            }
            let h = ProfileHelper::deserialize(deserializer)?;
            Ok(Profile {
                expected_rate: h.expected_rate,
                peak: h.peak,
                variance: h.variance,
            })
        }
    }

    impl<T: ResourceUnit + Serialize> Serialize for ThresholdSet<T> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeStruct;
            let mut s = serializer.serialize_struct("ThresholdSet", 3)?;
            s.serialize_field("warning", &self.warning)?;
            s.serialize_field("critical", &self.critical)?;
            s.serialize_field("floor", &self.floor)?;
            s.end()
        }
    }

    impl<'de, T: ResourceUnit + Deserialize<'de>> Deserialize<'de> for ThresholdSet<T> {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            #[derive(Deserialize)]
            struct ThresholdHelper<T> {
                warning: T,
                critical: T,
                floor: T,
            }
            let h = ThresholdHelper::deserialize(deserializer)?;
            Ok(ThresholdSet {
                warning: h.warning,
                critical: h.critical,
                floor: h.floor,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test domain
    fn test_domain() -> ConservationDomain<f64> {
        ConservationDomain::new(
            "test_domain",
            Budget {
                total: 100.0,
                allocated: 100.0,
                consumed: 0.0,
            },
            Profile {
                expected_rate: 10.0,
                peak: 20.0,
                variance: 0.1,
            },
            ThresholdSet {
                warning: 30.0,
                critical: 15.0,
                floor: 5.0,
            },
        )
    }

    #[test]
    fn test_budget_tracking() {
        let mut d = test_domain();

        assert_eq!(d.remaining(), 100.0);
        assert_eq!(d.budget.consumed, 0.0);

        // Tick some consumption
        let _ = d.tick(10.0);
        assert_eq!(d.budget.consumed, 10.0);
        assert!((d.remaining() - 90.0).abs() < f64::EPSILON);

        let _ = d.tick(20.0);
        assert_eq!(d.budget.consumed, 30.0);
        assert!((d.remaining() - 70.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_threshold_crossing_warning() {
        let mut d = test_domain();

        // Consume down to warning threshold (30.0)
        let _ = d.tick(30.0); // consumed = 30, remaining = 70
        let _ = d.tick(30.0); // consumed = 60, remaining = 40 — still > 30
        assert!(d.remaining() > 30.0 || d.remaining() == 30.0);

        // One more tick should cross into warning
        let _ = d.tick(10.0); // consumed = 70, remaining = 30
        assert_eq!(d.remaining(), 30.0); // exactly at warning

        // Cross warning
        let event = d.tick(5.0); // consumed = 75, remaining = 25 < 30
        assert!(event.is_some());
        if let Some(evt) = event {
            assert_eq!(evt.severity, Ternary::Neutral);
            match &evt.kind {
                EventKind::ThresholdCrossed { threshold_name, .. } => {
                    assert_eq!(*threshold_name, "warning");
                }
                other => panic!("Expected ThresholdCrossed, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_threshold_crossing_critical() {
        let mut d = test_domain();

        // Blast through to critical
        let _ = d.tick(50.0); // consumed=50, remaining=50
        let _ = d.tick(40.0); // consumed=90, remaining=10 < 15 (critical)

        let event = d.tick(0.0);
        assert!(event.is_some());
        if let Some(evt) = event {
            assert_eq!(evt.severity, Ternary::Negative);
            match &evt.kind {
                EventKind::ThresholdCrossed { threshold_name, .. } => {
                    assert_eq!(*threshold_name, "critical");
                }
                other => panic!("Expected ThresholdCrossed, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_budget_exceeded() {
        let mut d = test_domain();

        // Consume everything
        let _ = d.tick(50.0);
        let _ = d.tick(50.0); // consumed=100, remaining=0

        // Exceed
        let event = d.tick(1.0);
        assert!(event.is_some());
        if let Some(evt) = event {
            assert_eq!(evt.severity, Ternary::Negative);
            match &evt.kind {
                EventKind::BudgetExceeded { actual, limit } => {
                    assert_eq!(*actual, 101.0);
                    assert_eq!(*limit, 100.0);
                }
                other => panic!("Expected BudgetExceeded, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_empty_to_empty() {
        // An already-empty budget: tick should immediately report
        let mut d = ConservationDomain::new(
            "empty",
            Budget {
                total: 0_u32,
                allocated: 0,
                consumed: 0,
            },
            Profile {
                expected_rate: 0,
                peak: 0,
                variance: 0.0,
            },
            ThresholdSet {
                warning: 0,
                critical: 0,
                floor: 0,
            },
        );
        let event = d.tick(1);
        assert!(event.is_some());
    }

    #[test]
    fn test_rate_calculation() {
        let mut d = test_domain();
        assert_eq!(d.rate(), 0.0);

        d.tick(10.0);
        assert!((d.rate() - 10.0).abs() < 0.01);

        d.tick(20.0);
        assert!((d.rate() - 15.0).abs() < 0.01);

        d.tick(15.0);
        assert!((d.rate() - 15.0).abs() < 0.01);
    }

    #[test]
    fn test_project_remaining() {
        let mut d = test_domain();

        // No consumption = max duration
        assert_eq!(d.project_remaining(), Duration::MAX);

        d.tick(10.0);
        d.tick(10.0);

        let proj = d.project_remaining();
        assert!(proj > Duration::ZERO);
        // At rate=10, remaining=80 => 8 ticks
        assert_eq!(proj.as_secs(), 8);
    }

    #[test]
    fn test_history_actually_capped_at_1024() {
        // This genuinely exercises the 1024-entry cap on a *single* domain.
        // Once `remaining` drops to/below the floor, every subsequent tick —
        // even of zero — re-emits a floor event, so we can accumulate far more
        // than MAX_HISTORY events on one domain and verify trimming kicks in.
        let mut d = test_domain();
        // total=100, floor=5: consume 96 so remaining=4 <= 5  (floor event #1)
        let _ = d.tick(96.0);
        // remaining stays at 4, so each of these re-emits a floor event
        for _ in 0..2000 {
            let _ = d.tick(0.0);
        }
        assert!(
            d.history.len() <= 1024,
            "history must be capped at 1024, got {}",
            d.history.len()
        );
        assert!(
            d.history.len() > 512,
            "history should have accumulated many events, got {}",
            d.history.len()
        );
    }

    #[test]
    fn test_integer_resource_unit() {
        let mut d: ConservationDomain<u32> = ConservationDomain::new(
            "fuel_liters",
            Budget {
                total: 50,
                allocated: 50,
                consumed: 0,
            },
            Profile {
                expected_rate: 5,
                peak: 10,
                variance: 0.2,
            },
            ThresholdSet {
                warning: 15,
                critical: 8,
                floor: 3,
            },
        );

        assert_eq!(d.remaining(), 50);
        let _ = d.tick(5);
        assert_eq!(d.remaining(), 45);
        let _ = d.tick(5);
        assert_eq!(d.remaining(), 40);

        // Warning threshold
        let _ = d.tick(10); // remaining 30
        let _ = d.tick(10); // remaining 20
        let event = d.tick(10); // remaining 10 < 15
        assert!(event.is_some());
    }

    #[test]
    fn test_cascade_event() {
        // Cascade events are constructed manually for cross-domain scenarios
        let cascade: ConservationEvent<f64> = ConservationEvent {
            timestamp: Duration::from_secs(42),
            domain: "fuel",
            kind: EventKind::Cascade {
                trigger: "battery depleted, drawing from fuel reserve".into(),
                affected_domain: "battery",
            },
            severity: Ternary::Negative,
        };
        assert_eq!(cascade.domain, "fuel");
        match &cascade.kind {
            EventKind::Cascade {
                trigger,
                affected_domain,
            } => {
                assert!(trigger.contains("battery"));
                assert_eq!(*affected_domain, "battery");
            }
            _ => panic!("expected Cascade"),
        }
    }

    #[test]
    fn test_tick_count() {
        let mut d = test_domain();
        assert_eq!(d.tick_count(), 0);
        d.tick(10.0);
        assert_eq!(d.tick_count(), 1);
        d.tick(10.0);
        assert_eq!(d.tick_count(), 2);
    }

    #[test]
    fn test_clear_history() {
        let mut d = test_domain();
        // Force a threshold event
        let _ = d.tick(90.0);
        assert!(!d.history.is_empty());
        d.clear_history();
        assert!(d.history.is_empty());
    }

    /// The old threshold guard used `||` (OR), so an ordering where *only*
    /// `critical >= floor` held (but `warning < critical`) slipped through.
    /// This must now panic because the contract requires a total ordering.
    #[test]
    #[should_panic(expected = "thresholds must be ordered")]
    fn test_invalid_threshold_order_warn_below_critical_panics() {
        // warning=10 < critical=20 is invalid (warning must be >= critical).
        let _ = ConservationDomain::new(
            "bad",
            Budget {
                total: 100.0_f64,
                allocated: 100.0,
                consumed: 0.0,
            },
            Profile {
                expected_rate: 10.0,
                peak: 20.0,
                variance: 0.1,
            },
            ThresholdSet {
                warning: 10.0,
                critical: 20.0,
                floor: 5.0,
            },
        );
    }

    #[test]
    #[should_panic(expected = "thresholds must be ordered")]
    fn test_invalid_threshold_order_critical_below_floor_panics() {
        // critical=5 < floor=20 is invalid.
        let _ = ConservationDomain::new(
            "bad",
            Budget {
                total: 100.0_f64,
                allocated: 100.0,
                consumed: 0.0,
            },
            Profile {
                expected_rate: 10.0,
                peak: 20.0,
                variance: 0.1,
            },
            ThresholdSet {
                warning: 30.0,
                critical: 5.0,
                floor: 20.0,
            },
        );
    }
}

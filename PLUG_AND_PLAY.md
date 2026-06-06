# Ternary Conserve — Plug & Play Guide

Parametric conservation across resource domains: **fish stocks, fuel, battery, inference tokens, crew attention**.

## Philosophy

The conservation thesis is central to Ternary. Every measurable resource follows a closed-loop cycle:

```
Budget → Profile → Detect → Report
```

This isn't just monitoring — it's conservation with teeth. When a threshold is crossed, you get an event. When the budget is exceeded, you get a `Negative` severity event. When everything is nominal, silence.

## Quick Start

Add to `Cargo.toml`:

```toml
[dependencies]
ternary-conserve = "0.1"
ternary-types = "0.1"
```

Or from a local path:

```toml
[dependencies]
ternary-conserve = { path = "../ternary-conserve" }
ternary-types = { path = "../ternary-types" }
```

### Enable Serde (optional)

```toml
ternary-conserve = { version = "0.1", features = ["serde"] }
```

## Usage

### 1. Define a Domain

```rust
use ternary_conserve::{ConservationDomain, Budget, Profile, ThresholdSet};

let mut fuel = ConservationDomain::new(
    "fuel",
    Budget { total: 100.0, allocated: 100.0, consumed: 0.0 },
    Profile { expected_rate: 10.0, peak: 20.0, variance: 0.1 },
    ThresholdSet { warning: 30.0, critical: 15.0, floor: 5.0 },
);
```

### 2. Tick Consumption

```rust
// Normal consumption — no event
let event = fuel.tick(10.0);
assert!(event.is_none());

// Heavy draw
let event = fuel.tick(20.0);
assert!(event.is_none()); // peak is the max, not a threshold
```

### 3. React to Threshold Crossing

```rust
// Consume down to warning level
for _ in 0..5 { fuel.tick(10.0); }

if let Some(event) = fuel.tick(5.0) {
    match event.severity {
        Ternary::Neutral => println!("⚠️ {}: {}", event.domain, event.kind),
        Ternary::Negative => eprintln!("🔴 {}: {}", event.domain, event.kind),
        _ => {},
    }
}
```

### 4. Check Projections

```rust
let rate = fuel.rate();              // current consumption per tick
let eta = fuel.project_remaining();  // time until depletion
let rem = fuel.remaining();          // units remaining
```

## Domains

| Domain | Unit Type | Typical Values |
|--------|-----------|----------------|
| Fish stocks | `f64` / `u32` | Catch limits, biomass |
| Fuel | `f64` | Liters, gallons, range |
| Battery | `f64` / `u32` | mAh, state of charge |
| Inference tokens | `u64` | Tokens per LLM call |
| Crew attention | `u32` | Person-minutes per day |

## The Cycle in Detail

### Budget

```rust
pub struct Budget<T> {
    pub total: T,       // total resource grant
    pub allocated: T,   // how much was earmarked
    pub consumed: T,    // how much has been used
}
```

### Profile

```rust
pub struct Profile<T> {
    pub expected_rate: T,  // per-tick expected consumption
    pub peak: T,           // max per tick
    pub variance: f64,     // expected CV
}
```

### Detect

`tick(consumed)` checks:
1. Has the budget been exceeded? → `BudgetExceeded` event
2. Has the floor been hit? → `ThresholdCrossed(floor)` event
3. Has critical been crossed? → `ThresholdCrossed(critical)` event
4. Has warning been crossed? → `ThresholdCrossed(warning)` event
5. No thresholds crossed? → `None` (nominal)

### Report

Events carry:
- `timestamp` — when it happened
- `domain` — which domain
- `kind` — what kind of event
- `severity` — `Negative`(bad), `Neutral`(warn), `Positive`(healthy)

## Event Severity Mapping

| Condition | Severity |
|-----------|----------|
| Budget exceeded | `Negative` |
| Critical threshold | `Negative` |
| Floor hit | `Negative` |
| Warning threshold | `Neutral` |
| Rate anomaly | `Neutral` |
| Nominal | — (no event) |

## Cross-Domain Cascades

Cascade events allow one domain to signal another. Construct them manually:

```rust
let cascade = ConservationEvent {
    timestamp: Duration::from_secs(42),
    domain: "fuel",
    kind: EventKind::Cascade {
        trigger: "battery depleted, drawing from fuel reserve".into(),
        affected_domain: "battery",
    },
    severity: Ternary::Negative,
};
```

## Example: Fish Stock Monitoring

```rust
use ternary_conserve::*;

let mut fishery = ConservationDomain::new(
    "cod_stock",
    Budget { total: 500_000_u32, allocated: 450_000, consumed: 0 },
    Profile { expected_rate: 10_000, peak: 25_000, variance: 0.3 },
    ThresholdSet { warning: 100_000, critical: 50_000, floor: 20_000 },
);

// Simulate a fishing season
let catches = vec![12_000, 14_000, 9_000, 30_000, 11_000];
for catch in catches {
    if let Some(event) = fishery.tick(catch) {
        eprintln!("⚠️ Stock alert: {:?}", event);
    }
}

println!(
    "Season end: {} units remaining, depletion in {:?}",
    fishery.remaining(),
    fishery.project_remaining(),
);
```

## no_std Support

The crate is `#![no_std]` by default (uses `alloc`). No external dependencies required beyond `ternary-types`.

## License

MIT OR Apache-2.0

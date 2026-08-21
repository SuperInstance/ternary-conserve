# Ternary Conserve

Parametric conservation across resource domains.

**When to use this:** you have a finite, depletable budget (fuel, battery charge,
fish-stock biomass, LLM token quota, crew attention-hours) and you want to
*tick* consumption against it while automatically emitting events whenever a
threshold is crossed or the budget runs out. Use it when you need a single,
generic, `no_std`-friendly abstraction that turns "how much is left?" into
actionable `Negative`/`Neutral` severity signals — instead of hand-rolling a
 bespoke budget+alarm struct per resource type.

**When not to:** if you only need a plain counter, or need rich statistical
forecasting (this crate does deterministic threshold checks and a simple
rate-based depletion projection, not probabilistic prediction).

**The conservation thesis** — every measurable resource follows a closed-loop cycle:

```
Budget → Profile → Detect → Report
```

## Domains

- Fish stocks — catch limits, biomass thresholds
- Fuel — range management, trip planning
- Battery — mAh budgeting, charge cycles
- Inference tokens — LLM call budgets
- Crew attention — human-hours, meeting costs

## Quick Start

```toml
[dependencies]
ternary-conserve = "0.1"
ternary-types = "0.1"
```

```rust
use ternary_conserve::{ConservationDomain, Budget, Profile, ThresholdSet};

let mut fuel = ConservationDomain::new(
    "fuel",
    Budget { total: 100.0, allocated: 100.0, consumed: 0.0 },
    Profile { expected_rate: 10.0, peak: 20.0, variance: 0.1 },
    ThresholdSet { warning: 30.0, critical: 15.0, floor: 5.0 },
);

// Tick consumption — get events when thresholds are crossed
if let Some(event) = fuel.tick(10.0) {
    eprintln!("⚡ Event: {:?}", event);
}
```

See [`PLUG_AND_PLAY.md`](./PLUG_AND_PLAY.md) for full documentation.

## License

MIT

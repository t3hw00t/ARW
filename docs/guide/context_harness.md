---
title: Context Scenario Harness
---

# Context Scenario Harness

Updated: 2025-10-01
Status: Preview
Type: How-to

Microsummary:
- Exercise the context loop with deterministic fixtures that emit the same summaries and events as production.
- Shuffle steps via an optional Monte Carlo driver to stress coverage heuristics and timing boundaries.
- Enable the `context_harness` feature flag to expose the helpers in downstream tests or custom tools.
- Use the harness to replay tricky regressions before exposing changes to users.

The context loop now ships with a synthetic scenario harness so you can exercise
multi-iteration plans without stitching together live event streams. Use it to
stress coverage heuristics, replay known regressions, or dial Monte Carlo runs
around tricky iteration boundaries before changes reach users.

## What It Provides

- Deterministic fixtures for each iteration (working set summary, seeds, lane
  composition, coverage verdict) that drive the same summary and recall events
  emitted in production.
- Optional Monte Carlo driver that shuffles steps, injects timing jitter, and
  repeats a scenario against a seeded RNG for reproducible stress tests.
- Aggregated reporting so tests or ad-hoc scripts can see which coverage
  reasons triggered, where runs stopped, and which specs the auto-adjuster
  produced.

The harness lives behind `cfg(test)` and the `context_harness` feature flag. It
compiles automatically for unit tests but can also be pulled into custom tools
by enabling the feature.

```bash
# Run the built-in harness tests
cargo test -p arw-server context_loop::harness

# Build with the harness available to downstream crates
cargo build -p arw-server --features context_harness
```

## Anatomy of a Scenario

```rust
use arw_server::context_loop::harness as ctx;

let base_spec = working_set::WorkingSetSpec {
    query: Some("multi-agent synthesis".into()),
    embed: None,
    lanes: vec!["semantic".into(), "procedural".into()],
    limit: 6,
    expand_per_seed: 2,
    diversity_lambda: 0.7,
    min_score: 0.5,
    project: Some("demo".into()),
    lane_bonus: 0.1,
    scorer: Some("mmr".into()),
    expand_query: false,
    expand_query_top_k: 4,
    slot_budgets: Default::default(),
};

let first_pass = ctx::ScenarioSuccess::new("pass-1", fixture_low_coverage())
    .with_verdict(needs_more(vec!["below_target_limit"]))
    .with_next(ctx::NextSpec::AutoAdjust);

let second_pass = ctx::ScenarioSuccess::new("pass-2", fixture_full())
    .with_verdict(ctx::coverage::CoverageVerdict::satisfied());

let scenario = ctx::Scenario {
    name: "two-pass".into(),
    base_spec,
    steps: vec![
        ctx::ScenarioStep::Success(first_pass),
        ctx::ScenarioStep::Success(second_pass),
    ],
    max_iterations: 6,
    corr_id: Some("demo-corr".into()),
    monte_carlo: Some(ctx::MonteCarloSpec {
        runs: 10,
        shuffle_steps: true,
        jitter_ms: Some(20.0),
        seed: 42,
    }),
};

let report = scenario.run();
assert_eq!(report.failure_count(), 0);
```

Each iteration step can either:

- Return a successful fixture aligned with real working-set summaries, or
- Inject an error with a synthetic spec override to confirm recovery paths.

`NextSpec::AutoAdjust` mirrors the production auto-tuning logic, so scenarios
that flag coverage gaps automatically derive the next spec and keep the run
aligned with live behaviour.

## When to Use It

- **Regression coverage** – capture a failing iteration trace as fixtures and
  protect it with a unit test.
- **Monte Carlo probers** – explore edge cases (lane diversity, slot budgets)
  with controlled randomness before shipping new heuristics.
- **Documentation & demos** – render the emitted summaries/diagnostics in docs
  or notebooks without booting the entire server stack.

Because the harness emits the same payload shapes as the live context loop, you
can plug the resulting events into downstream consumers (dashboards, logging
pipes, snapshot tests) to verify their behaviour too.

## Extending the Harness

- Add new helper constructors in your own crate to build fixture libraries.
- Use `ScenarioReport::aggregate_reasons` to produce quick histograms of
  coverage gaps over many runs.
- Wrap `Scenario::run` inside integration tests or scripts to snapshot the
  final spec and working set for human review.

Pull requests that add more built-in fixtures or Monte Carlo utilities are
welcome—just keep them pure data transforms so the harness stays deterministic
and lightweight.

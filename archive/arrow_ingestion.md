---
title: Arrow Ingestion Prototype
---

# Arrow Ingestion Prototype
{ .topic-trio style="--exp:.5; --complex:.6; --complicated:.8" data-exp=".5" data-complex=".6" data-complicated=".8" }

A prototype module was added to `arw-core` demonstrating JSON ingestion with the [`arrow2`](https://crates.io/crates/arrow2) crate. Two paths were compared:

Updated: 2025-09-12
Type: How‑to

- `serde_json` parsing into native structs
- `arrow2` conversion into columnar arrays

## Benchmark

```text
serde_json  time:   [68.095 µs 68.524 µs 68.968 µs]
arrow2      time:   [440.50 µs 443.06 µs 445.90 µs]
```

For 1,000 records, `arrow2` is ~6x slower than direct `serde_json` parsing. At this scale, arrow2 doesn't provide immediate performance benefits.

## Recommendation

Continue using existing serialization for now. Revisit `arrow2` once datasets grow larger or when columnar memory layouts are required for downstream analytics.

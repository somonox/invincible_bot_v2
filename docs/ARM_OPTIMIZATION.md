# ARM64 optimization (2026-09-20)

Host: Oracle ARM64 VM, four Neoverse-N1 cores, Rust 1.93.1. Baseline: f55b03e.
All experiments ran offline, without bot credentials or network gameplay, in
separate build directories. The live user service was already inactive and was
kept inactive. `perf` sampling was denied by the host's perf_event_paranoid=4;
no host permissions/settings were changed. These are elapsed search timings,
not a sampled attribution of CPU time to individual functions.

## Changes

- Six movement neighbors use a stack array instead of a heap allocation per BFS
  vertex. A per-thread workspace reuses the queue and placement buffers; epoch
  visitation avoids clearing the full visit table on every search.
- A bounded, 512-slot per-thread movement cache reuses SRS-X results. It checks
  exact board rows, width, piece and spin mode after hashing. Collisions only
  evict an entry, never merge unequal positions. Movement/spin ordering is kept.
- Row bit operations calculate holes, covered cells and transitions. Heights
  use a fixed array and first occupied bits, avoiding a temporary heap vector.
  The 16-column full-row mask also avoids a u16 shift overflow.
- Beam selection uses partial selection rather than sorting every candidate
  repeatedly. Original insertion order and primary-ranking tie breaks are
  explicit. The 32/16/16 search allocation and final move choices are preserved.
- ARM server builds use the local CPU target, ThinLTO and one codegen unit.
- Optional `arm-neon` evaluates eight rows per 128-bit vector for transition
  counts. Only integer operations change; floating-point model accumulation
  remains untouched. Width 16 and non-NEON targets use the scalar path. Manual
  NEON is not enabled by default because its additional gain was marginal.

The preview queue representation and model/training policy were not changed.
No depth or beam-width increase is included in these measurements.

## Measurements

Each main run contains four fixed streams of 80 placements, depth six and a
three-cell residue. Three fresh-process repetitions rotate variant order to
reduce ordering bias. The table reports the median mean and median p95 across
those three runs. All binaries use the same instrumented search_bench source.

| Variant | Mean search (ms) | p95 (ms) |
| --- | ---: | ---: |
| Baseline | 15.03 | 17.01 |
| Algorithm/data changes, generic ARM build | 8.41 | 10.14 |
| Plus N1 target + ThinLTO + one codegen unit (default deployment) | 8.06 | 9.79 |
| Plus manual NEON (optional) | 8.03 | 9.69 |

The default reduces mean time by 46.4% (about 1.86x search throughput) in this
sample. NEON adds only about 0.3% to the mean; that is not strong evidence of a
material end-to-end gain on N1. This is not an estimate of live PPS or win rate.

The choice hash, per-game statistics, attack and strategy labels were identical
across all four variants: 960 main-run decisions and another 240 decisions in
empty-board hybrid, pure Combo and pure PC checks per variant. Additional tests
compare all board metrics with cell-by-cell implementations on 1,600 boards of
widths 1-16, compare partial selection with the old stable sort (including ties),
exercise cache key dimensions/order and epoch wrap, and retain all prior SRS-X,
PC, cancellation, multiplier and GUI tests. ARM runs enable NEON for its tests.

Raw data: [arm_optimization.json](benchmarks/arm_optimization.json).

## Reproduction

Prepare separate baseline/optimized source directories. Copy the current
`examples/search_bench.rs` to both: its only semantic addition is a deterministic
move-sequence hash. In each directory, build the generic variant with:

```sh
cargo build --locked --release --no-default-features --example search_bench
```

For the N1 variant use a separate target directory:

```sh
CARGO_TARGET_DIR=target/n1 CARGO_PROFILE_RELEASE_LTO=thin \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 RUSTFLAGS='-C target-cpu=neoverse-n1' \
cargo build --locked --release --no-default-features --example search_bench
```

Copy that binary aside, then repeat with `--features arm-neon`. Compare the four
saved binaries in the order baseline, generic optimized, N1, N1+NEON:

```sh
python3 tests/benchmark_optimizations.py /path/baseline /path/generic /path/n1 /path/neon
```

The runner rejects differing moves or game results before emitting its report.

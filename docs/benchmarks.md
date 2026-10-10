# Benchmarks

Run benchmarks in the Nix development shell. `cargo bench` uses the optimized profile and [Criterion](https://criterion-rs.github.io/book/), with fixture setup and behavior checks outside the timed closures.

```bash
cargo bench --bench performance
cargo bench --bench performance -- 'render/'
cargo test --bench performance
```

Reports and saved baselines live under `target/criterion/`, respecting `CARGO_TARGET_DIR`. Set `CRITERION_HOME` to choose another location.

## CI reports

CI runs smoke checks without performance thresholds. The `Benchmarks` workflow compares a PR against its common ancestor, compiling each revision's own harness in the head's Nix environment. It also accepts a baseline revision for manual runs.

Each revision has a separate Criterion report with raw samples, logs, revision SHAs and environment details, retained for 14 days. Check workload inputs and timing boundaries before comparing them. Timings are advisory because shared runners are noisy.

## Workloads

| Group            | Measurement                                                                                           |
| ---------------- | ----------------------------------------------------------------------------------------------------- |
| `shortcodes`     | Emoji / icon replacement in ordinary text and unmatched colon prefixes                                |
| `render_options` | Option extraction from small and large params tables                                                  |
| `render`         | Prose and code fences with preloaded templates, syntax, and image resolver                            |
| `syntax/load`    | Loading and destroying a fresh syntax set                                                             |
| `images`         | Generated PNG processing with a fresh cache, or lookup in a primed cache                              |
| `build`          | CLI startup, prior-output cleanup, and complete builds with default, large-params, or minified output |

Cold-image samples exclude image creation and resolver construction, but include processing and cache release. Build fixtures use minimal templates and warm filesystem caches, with search and Git timestamps disabled. Measure representative sites separately when evaluating theme or Pagefind costs.

## Comparing revisions

Keep measured inputs, timing boundaries, and the toolchain consistent. Run sequentially on an idle host and repeat noisy comparisons. For a local change that preserves these contracts:

```bash
cargo bench --bench performance -- --save-baseline before
# Apply the change while preserving the workload.
cargo bench --bench performance -- --baseline before
```

For API changes or separate worktrees, use the `Benchmarks` workflow so each revision compiles its own harness. Review the benchmark changes to establish which workloads remain comparable. Include the revisions, command, compiler, profile, fixture parameters, host conditions, and Criterion confidence intervals in performance PRs.

## Adding workloads

Add a module under `crates/kiln/benches/performance/` and register it in `performance.rs`. Reuse fixtures and public APIs, validate behavior before timing, and keep setup outside the timed closure. Separate cold initialization from cached work. Use the build group's flat sampling for expensive cases.

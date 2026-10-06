# Benchmarks

Run benchmarks in the Nix development shell. `cargo bench` uses the repository's optimized profile and [Criterion](https://criterion-rs.github.io/book/). Fixture setup and behavior checks are outside the timed closures. CI runs smoke checks without performance thresholds.

```bash
cargo bench --bench performance
cargo bench --bench performance -- 'render/'
cargo test --bench performance
```

Reports and saved baselines live under `criterion/` beside the compiled profile directory, normally `target/criterion/`. Set `CRITERION_HOME` to choose another location.

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

Keep the workload source and toolchain identical, run sequentially on an idle host, and repeat noisy comparisons.

```bash
cargo bench --bench performance -- --save-baseline before
# Apply the change while preserving the workload.
cargo bench --bench performance -- --baseline before
```

For separate worktrees, use separate Cargo target directories and copy the baseline's `criterion/` directory into the second target directory. Include the revision, command, compiler, profile, fixture parameters, host conditions, and Criterion confidence intervals in performance PRs. Keep raw samples and logs local.

## Adding workloads

Add a module under `crates/kiln/benches/performance/` and register it in `performance.rs`. Reuse fixtures and public APIs, validate behavior before timing, and keep setup outside the timed closure. Separate cold initialization from cached work. Use the build group's flat sampling for expensive cases.

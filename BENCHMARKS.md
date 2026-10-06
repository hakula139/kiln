# Benchmarks

Run benchmarks in the Nix development shell. `cargo bench --bench performance` uses [Criterion](https://criterion-rs.github.io/book/) with the repository's optimized profile. Fixture creation and output checks happen before sampling. Reports and saved samples live under `criterion/` beside the compiled benchmark's profile directory, normally `target/criterion/`. A configured Cargo target directory or explicit target triple also determines the report location.

```bash
cargo bench --bench performance
cargo bench --bench performance -- 'render/'
cargo test --bench performance
```

The test command runs the registered cases without collecting performance samples and checks fixture output. CI runs this smoke check. Timings run locally because host load makes CI speed thresholds unreliable.

| Group            | Workload                                                                              | Included in the measurement                                                                       |
| ---------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `shortcodes`     | Ordinary text and 5,000 unmatched colon prefixes, followed by one valid emoji or icon | Replacement and output allocation                                                                 |
| `render_options` | One supported setting and zero or 1,000 unrelated settings                            | Render option extraction from an existing params table                                            |
| `render`         | Twenty prose sections or twenty Rust code fences                                      | Page rendering with a preloaded syntax set, templates, and image resolver                         |
| `syntax/load`    | Bundled syntax definitions                                                            | Loading and destroying a fresh syntax set                                                         |
| `images/cold`    | A generated 1,024 × 768 PNG with a fresh resolver                                     | Path resolution, decoding, resizing, WebP encoding, result allocation, and resolver cache release |
| `images/cached`  | The same image with a primed resolver                                                 | Path resolution and cached metadata lookup                                                        |
| `build`          | 20 or 200 posts with sections, tags, pagination, feeds, static CSS / JS, and sitemap  | CLI process startup and all build stages, including removal of prior output                       |

The build group also measures 200 posts with minification and 200 posts with 1,000 unrelated string settings in `[params]`. These are separate cases, so extra template settings and minification can be compared with the default 200-post workload. Search indexing and Git timestamps are disabled. The generated sites use minimal templates and warm filesystem caches. They do not estimate production-theme rendering or Pagefind costs.

Image creation and resolver construction are excluded from the cold-image sample. Each sample receives an empty resolver cache. This measures fresh image processing within a build, while the cached case measures reuse within the same build. Full CLI builds always create a fresh resolver.

Build subprocess output is discarded during measurement. A failed build fails the benchmark. Before sampling, the harness checks every post's title and content, the generated HTML count, feed membership, and copied assets. Shortcode cases assert exact output, render cases verify heading text, link targets, and highlighted source text, and image cases verify dimensions and the placeholder.

## Comparing revisions

Keep the workload source and toolchain identical on both revisions. A saved baseline is local to its target directory. For separate worktrees, use separate build target directories and copy the baseline's `criterion/` directory into the second target directory. Run measurements sequentially so the two builds do not compete for resources.

```bash
cargo bench --bench performance -- --save-baseline before
# Apply the change being measured, preserving the benchmark workload.
cargo bench --bench performance -- --baseline before
```

Criterion reports confidence intervals and relative changes. Inspect the raw samples under `target/criterion/` when a result is noisy. Repeat comparisons in alternating order and keep the host idle during sampling. Microbenchmark gains apply to their defined inputs. Confirm effects on representative complete sites separately.

Capture the revision, local modifications, compiler, platform, and complete output with a report:

```bash
set -o pipefail
mkdir -p target/benchmark-reports
{
  git rev-parse HEAD
  git status --short
  git diff --stat
  rustc -Vv
  uname -a
  cargo bench --bench performance -- --save-baseline reference
} 2>&1 | tee target/benchmark-reports/reference.txt
```

Include the command, benchmark revision, target profile, fixture parameters, and host conditions in a performance PR. Reports are local artifacts. Commit the harness and summarized evidence, keeping machine-specific samples out of Git.

## Adding workloads

Add a focused module under `crates/kiln/benches/performance/` and register its `benchmarks` function in `performance.rs`. Use existing public APIs and share fixture setup where the workload needs the same site contract. Put input preparation and behavior assertions outside the timed closure, black-box inputs when needed, and declare byte or element throughput when its units are meaningful. Give distinct names to cold initialization and cached work. For expensive cases, use flat sampling and a smaller sample count as the build group does.

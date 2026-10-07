import argparse
import json
import os
from pathlib import Path
import tomllib


def read_revision(root):
    estimates = {}
    for path in (root / "criterion").glob("**/measured/benchmark.json"):
        benchmark = json.loads(path.read_text())
        identifier = benchmark["full_id"]
        if identifier in estimates:
            raise ValueError(f"Duplicate benchmark result: {identifier}")
        estimate = json.loads((path.parent / "estimates.json").read_text())
        estimates[identifier] = estimate["mean"]
    if not estimates:
        raise ValueError(f"No benchmark measurements in {root}")

    manifest = root / "workloads.toml"
    workloads = {}
    if manifest.exists():
        for workload in tomllib.loads(manifest.read_text())["workloads"]:
            identifier = workload["id"]
            if identifier in workloads:
                raise ValueError(f"Duplicate workload: {identifier}")
            workloads[identifier] = (workload["contract"], workload["inputs"])
        if workloads.keys() != estimates.keys():
            raise ValueError(f"Workload manifest differs from measured cases in {root}")
    return estimates, workloads


def report(base, head):
    base_estimates, base_workloads = base
    head_estimates, head_workloads = head
    lines = [
        "## Benchmark comparison",
        "",
        "Each revision uses its own API adapters. Changes are compared only when the workload ID, input fingerprint, and measurement contract match. Timings on shared runners are advisory. Means and their Criterion confidence intervals are in nanoseconds. Raw measurements and workload manifests are in the artifact.",
        "",
        "| Workload | Base mean (95% CI) | Head mean (95% CI) | Mean change |",
        "| --- | ---: | ---: | --- |",
    ]
    for identifier in sorted(base_estimates.keys() | head_estimates.keys()):
        before = base_estimates.get(identifier)
        after = head_estimates.get(identifier)
        if before is None:
            change = "New workload"
        elif after is None:
            change = "Removed workload"
        elif identifier not in base_workloads or identifier not in head_workloads:
            change = "Compatibility manifest unavailable"
        elif base_workloads[identifier] != head_workloads[identifier]:
            change = "Inputs or measurement contract changed"
        else:
            change = f"{100 * (after['point_estimate'] / before['point_estimate'] - 1):+.2f}%"
        lines.append(
            f"| `{identifier}` | {format_estimate(before)} | {format_estimate(after)} | {change} |"
        )
    return "\n".join(lines) + "\n"


def format_estimate(estimate):
    if estimate is None:
        return "-"
    interval = estimate["confidence_interval"]
    return f"{estimate['point_estimate']:.1f} ({interval['lower_bound']:.1f}–{interval['upper_bound']:.1f})"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("results", type=Path)
    args = parser.parse_args()
    summary = report(
        read_revision(args.results / "base"), read_revision(args.results / "head")
    )
    (args.results / "comparison.md").write_text(summary)
    if path := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(path, "a") as output:
            output.write(summary)
    print(summary)


if __name__ == "__main__":
    main()

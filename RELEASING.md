# Releasing

Use the [release skill](https://github.com/hakulab/plugins/blob/main/plugins/hakula-workflow/skills/release/SKILL.md) and [devkit's release guide](https://github.com/hakulab/devkit/blob/v3/docs/releasing.md) for the shared release procedure. This document defines kiln's release contract.

## Rust toolchain maintenance

The Nix package uses the Rust compiler from the locked nixpkgs input. The development shell and release workflow use `rust-toolchain.toml`. Update that pin manually so its compiler and the locked `rust-overlay` stay in sync.

1. Refresh the `rust-overlay` input with `nix flake update rust-overlay`.
2. Choose a newer stable version available in the updated overlay and set `channel` in `rust-toolchain.toml` to that version.
3. Confirm `nix develop -c rustc --version` prints the chosen version and run `nix develop -c cargo check --locked --all-targets --all-features`.
4. Open one PR with the `rust-toolchain.toml` change. If the refresh changes `flake.lock`, commit both files together.

## Release preparation

Update `workspace.package.version` in `Cargo.toml` and run `cargo build` to refresh `Cargo.lock`. Draft `CHANGELOG.md` with [devkit's changelog procedure](https://github.com/hakulab/devkit/blob/v3/docs/releasing.md#draft-a-changelog).

When npm publication is enabled with `NPM_PUBLISH_ENABLED=true`, confirm that both launchers and their four platform packages trust GitHub Actions for `hakulab/kiln` through `release.yml`. Checking with `npm trust list @kiln-ssg/<package>` requires a local npm login. Add a missing publisher through the package's npm settings or with:

```bash
npm trust github @kiln-ssg/<package> --repo hakulab/kiln --file release.yml
```

## Verify publication

The Release workflow builds and tests base and extended variants before publishing. Check the GitHub release notes, all four platform archives and their checksum sidecars, and the tagged Nix package's version.

npm publication runs only when `NPM_PUBLISH_ENABLED` is `true`. Verify all six packages and each launcher-installed version. Stable releases use `latest`, and prereleases use `next`.

Inspect npm provenance and confirm the attestation identifies this repository's `.github/workflows/release.yml` and release tag:

```bash
npm view @kiln-ssg/kiln@X.Y.Z dist.attestations
```

Successful publication notifies the [Homebrew tap](https://github.com/hakulab/homebrew-tap#maintain-formulae). Check its formula update PR.

## Recovery

Follow the release skill and devkit's recovery procedure. npm versions are immutable. Rerunning a failed Publish npm job skips packages already published and publishes the remainder.

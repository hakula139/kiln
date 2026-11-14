# Releasing

Use the [release skill](https://github.com/hakula139/plugins/blob/main/plugins/hakula-workflow/skills/release/SKILL.md) and [devkit's release guide](https://github.com/hakula139/devkit/blob/v3/docs/releasing.md) for the shared release procedure. This document defines kiln's release contract.

## Release preparation

Update `workspace.package.version` in `Cargo.toml` and run `cargo build` to refresh `Cargo.lock`. Draft `CHANGELOG.md` with [devkit's changelog procedure](https://github.com/hakula139/devkit/blob/v3/docs/releasing.md#draft-a-changelog).

When npm publication is enabled with `NPM_PUBLISH_ENABLED=true`, confirm that both launchers and their six platform packages trust GitHub Actions for `hakula139/kiln` through `release.yml`. Checking with `npm trust list @kiln-ssg/<package>` requires a local npm login. Add a missing publisher through the package's npm settings or with:

```bash
npm trust github @kiln-ssg/<package> --repo hakula139/kiln --file release.yml
```

## Verify publication

The Release workflow builds and tests base and extended variants before publishing. Check the GitHub release notes, all six platform archives and their checksum sidecars, and the tagged Nix package's version.

npm publication runs only when `NPM_PUBLISH_ENABLED` is `true`. Verify all eight packages and each launcher-installed version. Stable releases use `latest`, and prereleases use `next`.

Inspect npm provenance and confirm the attestation identifies this repository's `.github/workflows/release.yml` and release tag:

```bash
npm view @kiln-ssg/kiln@X.Y.Z dist.attestations
```

## Recovery

Follow the release skill and devkit's recovery procedure. npm versions are immutable. Rerunning a failed Publish npm job skips packages already published and publishes the remainder.

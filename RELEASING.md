# Releasing

Follow the [shared release procedure](https://github.com/hakula139/devkit/blob/61682dd34d15732234bac835f3603de8de5b86f1/docs/releasing.md). Prepare a release PR, merge it after approval and passing checks, then separately obtain approval to tag its squash commit.

## Release preparation

Update `workspace.package.version` in `Cargo.toml` and run `cargo build` to refresh `Cargo.lock`. Use this repository's `cliff.toml` for `CHANGELOG.md`. Add the new version's compare reference above the existing footer links, using the previous stable tag for stable releases and the previous tag for prereleases.

Cargo dependencies that fix an advisory or affect shipped behavior use `fix(deps)` in PR titles. The root `package.json` contains development tools, whose updates remain `chore`.

Confirm that all eight npm packages trust GitHub Actions for `hakula139/kiln` through `release.yml`. Checking or configuring npm trust requires a local npm login:

```bash
for variant in kiln kiln-extended; do
  for package in "$variant" "$variant"-{darwin-arm64,linux-x64-gnu,win32-x64-msvc}; do
    npm trust list "@kiln-ssg/$package"
  done
done
```

Add a missing publisher through the package's npm settings or with:

```bash
npm trust github @kiln-ssg/<package> --repo hakula139/kiln --file release.yml
```

## Verify publication

The Release workflow builds and tests base and extended variants before publishing. Check its GitHub release notes, archives, and checksums.

npm publication runs only when `NPM_PUBLISH_ENABLED` is `true`. Verify all eight packages and each launcher-installed version. Stable releases use `latest`, and prereleases use `next`.

Inspect npm provenance and confirm the attestation identifies this repository's `.github/workflows/release.yml` and release tag:

```bash
npm view @kiln-ssg/kiln@X.Y.Z dist.attestations
```

npm versions are immutable. Rerunning a failed Publish npm job skips packages already published and publishes the remainder.

# Releasing

## Changelog

[git-cliff](https://git-cliff.org) drafts each release's `CHANGELOG.md` section from `cliff.toml`.

- Each squash-merged PR title becomes one entry, so write titles for users.
- `feat`, `fix`, `perf`, and `revert` titles appear under Added, Fixed, Changed, and Removed, respectively. Titles with `!` appear under Breaking changes. All other types are skipped, including routine `chore(deps)` and `chore(flake)` bumps. Use `fix(deps): ...` for a Cargo bump that fixes an advisory or changes behavior in the shipped `kiln`. The root `package.json` holds only development tools, so their bumps remain `chore`, including advisory fixes.
- Within each group, unscoped entries come first, then the rest sorted by scope, so related entries sit together. Each `(#N)` links to its PR.

## Prepare a release

1. Update `workspace.package.version` in `Cargo.toml` and run `cargo build` to refresh `Cargo.lock`.
2. Prepend the changelog section with [git-cliff](https://git-cliff.org):

   ```bash
   git cliff --unreleased --tag vX.Y.Z --prepend CHANGELOG.md
   ```

   For a prerelease, include prerelease tags as boundaries:

   ```bash
   git cliff --unreleased --tag vX.Y.Z-alpha.N --tag-pattern '^v[0-9]+\.' --prepend CHANGELOG.md
   ```

   `--prepend` preserves previous sections. `--output CHANGELOG.md` replaces the file. Review the generated section.
3. `--prepend` writes only the new section, so add `[X.Y.Z]: https://github.com/hakula139/kiln/compare/<previous-tag>..vX.Y.Z` above the existing footer links by hand. Use the previous stable tag for a stable release and the previous tag for a prerelease.
4. Confirm that all eight npm packages trust the Release workflow, which publishes them through npm trusted publishing. `npm trust` needs an npm login, so run it locally:

   ```bash
   for variant in kiln kiln-extended; do
     for package in "$variant" "$variant"-{darwin-arm64,linux-x64-gnu,win32-x64-msvc}; do
       npm trust list "@kiln-ssg/$package"
     done
   done
   ```

   Each package should list GitHub Actions for `hakula139/kiln` with the `release.yml` workflow. Add a missing entry with `npm trust github @kiln-ssg/<package> --repo hakula139/kiln --file release.yml`, or in the package settings on the npm website.
5. Verify that the diff contains only the version and changelog updates, then commit it as `chore(release): vX.Y.Z`. Pushing the tag starts the Release workflow and is hard to undo, so get approval for the tag push separately from the version bump:

   ```bash
   git tag vX.Y.Z
   git push origin main
   git push origin vX.Y.Z
   ```

## Verify publication

Watch the [Release workflow](https://github.com/hakula139/kiln/actions/workflows/release.yml) and check:

- GitHub release notes, base / extended archives and their checksums.
- npm packages and the version installed by each entry package. Stable releases use `latest`, and prereleases use `next`. The workflow publishes to npm only when the repository variable `NPM_PUBLISH_ENABLED` is `true`.
- npm provenance, starting with `npm view @kiln-ssg/kiln@X.Y.Z dist.attestations`. Verify that the attestation identifies this repository's `.github/workflows/release.yml` workflow and the release tag.

npm versions are immutable, so rerunning a failed Publish npm job skips the packages that are already published and publishes the rest.

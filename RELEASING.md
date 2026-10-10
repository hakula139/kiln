# Releasing

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
3. Add `[X.Y.Z]: https://github.com/hakula139/kiln/compare/<previous-tag>..vX.Y.Z` above the existing footer links. Use the previous stable tag for a stable release and the previous tag for a prerelease.
4. Verify the version and changelog diff, then commit it as `chore(release): vX.Y.Z`. Once the release is approved, tag and push that commit:

   ```bash
   git tag vX.Y.Z
   git push origin main
   git push origin vX.Y.Z
   ```

Watch the [Release workflow](https://github.com/hakula139/kiln/actions/workflows/release.yml) and verify the published assets. Stable npm releases use `latest`, and prereleases use `next`. Published npm versions are immutable, so inspect completed publication jobs before retrying a failed release.

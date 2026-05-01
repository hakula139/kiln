# Releasing

Releases are produced by `.github/workflows/release.yml`, triggered when a tag matching `v[0-9]+.*` is pushed.

## Standard release

1. Update `CHANGELOG.md`: move `[Unreleased]` content under a new `[X.Y.Z] - YYYY-MM-DD` section, add a fresh empty `[Unreleased]` section, update the link references at the bottom.
2. Bump version in `Cargo.toml` (`workspace.package.version`).
3. Run `cargo build` to refresh `Cargo.lock`.
4. Commit: `chore(release): vX.Y.Z`.
5. Tag and push:

   ```bash
   git tag vX.Y.Z
   git push origin main
   git push origin vX.Y.Z
   ```

6. The workflow creates the GitHub Release, extracting the matching `[X.Y.Z]` section from `CHANGELOG.md` as release notes, and uploads:
   - `kiln-x86_64-unknown-linux-gnu.tar.gz` (+ `.sha256`)
   - `kiln-aarch64-apple-darwin.tar.gz` (+ `.sha256`)

## Re-cutting an existing tag

If a release needs to be redone (e.g., bad assets):

```bash
gh release delete vX.Y.Z --cleanup-tag --yes
git tag vX.Y.Z   # re-tag locally on the desired commit
git push origin vX.Y.Z
```

Or trigger `workflow_dispatch` from the Actions tab against the existing tag.

## Targets

Currently `x86_64-unknown-linux-gnu` (CI consumers) and `aarch64-apple-darwin` (macOS dev). Add new targets by extending the matrix in `release.yml`.

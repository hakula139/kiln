# Releasing

Releases are produced by `.github/workflows/release.yml`, triggered when a tag matching `v[0-9]+.*` is pushed.

`CHANGELOG.md` is generated from Conventional Commits via [`git-cliff`](https://git-cliff.org). The `cliff.toml` config groups commits into Keep a Changelog sections (`Added`, `Fixed`, `Changed`, `Removed`, `Dependencies`) and is the single source of truth for both the in-repo changelog and GitHub Release notes (the `taiki-e/create-gh-release-action` step extracts the matching version section).

## Standard release

1. Bump version in `Cargo.toml` (`workspace.package.version`).
2. Run `cargo build` to refresh `Cargo.lock`.
3. Regenerate `CHANGELOG.md` from commits:

   ```bash
   git cliff --tag vX.Y.Z --output CHANGELOG.md
   ```

   Review the output and edit by hand if any bullet needs polishing — the file you commit is what users will read.
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

## Installing `git-cliff`

```bash
brew install git-cliff       # macOS / Homebrew
cargo install git-cliff      # any platform with cargo
```

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

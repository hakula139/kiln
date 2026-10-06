# Releasing

Releases are produced by `.github/workflows/release.yml`, triggered when a tag matching `v[0-9]+.*` is pushed.

`CHANGELOG.md` sections are generated from Conventional Commits via [`git-cliff`](https://git-cliff.org). The `cliff.toml` config groups commits into Keep a Changelog sections (`Breaking changes`, `Added`, `Fixed`, `Changed`, `Removed`, `Dependencies`). GitHub Release notes use the matching section through `taiki-e/create-gh-release-action`.

Stable releases include all changes since the previous stable tag, including prereleases. Prerelease sections contain changes since the previous tag. Existing sections remain in the changelog when a stable summary is added.

Any prose that should land in the changelog must come from a commit message: use `feat!:` / `fix!:` (or `feat(scope)!:` etc.) on PRs that introduce breaking changes so they surface in the `Breaking changes` section. Inline HTML in commit subjects is auto-backticked by a `commit_preprocessors` rule, so a subject like `feat(render)!: wrap <img> in <span class="lqip">` renders correctly in the changelog without manual escaping.

## Standard release

1. Bump version in `Cargo.toml` (`workspace.package.version`).

2. Run `cargo build` to refresh `Cargo.lock`.

3. Prepend the new changelog section. For a stable release:

   ```bash
   git cliff --unreleased --tag vX.Y.Z --prepend CHANGELOG.md
   ```

   For a prerelease, include prerelease tags as boundaries:

   ```bash
   git cliff --unreleased --tag vX.Y.Z-rc.N --tag-pattern '^v[0-9]+\.' --prepend CHANGELOG.md
   ```

   `--prepend` preserves existing sections. Use it when adding a release.

   Inspect the diff to confirm the new section reads well. Correct underlying commit messages and regenerate when needed. Do not hand-edit generated body sections.

4. Add the compare-link footer line manually. `--prepend` does not touch the footer block. Insert `[X.Y.Z]: https://github.com/hakula139/kiln/compare/<prev-tag>..vX.Y.Z` above the previous-version line. Use the previous stable tag for a stable release and the previous tag for a prerelease.

5. Commit: `chore(release): vX.Y.Z`.

6. Tag and push:

   ```bash
   git tag vX.Y.Z
   git push origin main
   git push origin vX.Y.Z
   ```

7. The workflow creates the GitHub Release from the matching changelog section and uploads archives for the [supported targets](#targets), with SHA-256 checksums.

## Installing `git-cliff`

```bash
brew install git-cliff       # macOS / Homebrew
cargo install git-cliff      # any platform with cargo
```

## Re-cutting an existing tag

If a release needs to be redone (e.g., bad assets):

```bash
gh release delete vX.Y.Z --cleanup-tag --yes
git tag vX.Y.Z               # re-tag locally on the desired commit
git push origin vX.Y.Z
```

Or trigger `workflow_dispatch` from the Actions tab against the existing tag.

## Targets

Three platforms ship per release:

- `x86_64-unknown-linux-gnu` (Linux CI consumers, `ubuntu-latest`)
- `aarch64-apple-darwin` (Apple Silicon dev, `macos-latest`)
- `x86_64-pc-windows-msvc` (Windows, `windows-latest`; dav1d sourced from vcpkg + pkgconfiglite)

Add new targets by extending the matrix in `release.yml`. `libdav1d` must be reachable via `pkg-config` on the host, since the `image` crate's `avif-native` feature pulls in `dav1d-sys`.

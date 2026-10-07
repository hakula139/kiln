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

## Rebuilding release assets

To replace faulty archives for an existing release, run the current workflow against its unchanged tag:

```bash
gh workflow run release.yml --ref main -f tag=vX.Y.Z
```

The workflow builds the tagged source with the selected workflow revision's packaging tools, verifies all archives, and replaces the existing assets and checksums. It preserves the tag, release notes, and source commit. Source changes require a new version.

## Targets

Three platforms ship per release:

- `x86_64-unknown-linux-gnu` (Ubuntu 24.04 or compatible glibc)
- `aarch64-apple-darwin` (Apple Silicon)
- `x86_64-pc-windows-msvc` (Windows)

Official binaries enable all features, including AVIF decoding. The shared native-dependency action builds a checksum-pinned dav1d static library in an isolated prefix. Each archive is extracted and checked for native dependencies, then used to build a site containing an AVIF image with the build library unavailable. Publication waits for every platform's checks to pass. Extend the matrix and dependency checks together when adding targets.

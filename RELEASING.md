# Releasing

Releases are produced by `.github/workflows/release.yml`, triggered when a tag matching `v[0-9]+.*` is pushed.

`CHANGELOG.md` sections are generated from Conventional Commits via [`git-cliff`](https://git-cliff.org). The `cliff.toml` config groups commits into Keep a Changelog sections (`Breaking changes`, `Added`, `Fixed`, `Changed`, `Removed`, `Dependencies`). GitHub Release notes use the matching section extracted by `parse-changelog`.

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

7. After all [supported targets](#targets) build successfully, the workflow creates the GitHub Release from the matching changelog section and attaches the archives and SHA-256 checksums. Existing releases are preserved if the workflow is rerun.

## Installing `git-cliff`

```bash
brew install git-cliff       # macOS / Homebrew
cargo install git-cliff      # any platform with cargo
```

## Targets

Three platforms ship per release:

- `x86_64-unknown-linux-gnu` (Linux)
- `aarch64-apple-darwin` (Apple Silicon)
- `x86_64-pc-windows-msvc` (Windows)

Each platform has two archives, both containing the `kiln` binary. `kiln-<target>` uses the default Cargo features and requires no dav1d installation. `kiln-extended-<target>` enables AVIF placeholder decoding. The extended Linux binary needs `libdav1d` from the system package manager. On macOS, install it with `brew install dav1d`. Windows extended binaries link dav1d statically through vcpkg.

AVIF files are published and their dimensions are read in both variants. See [Building from Source](README.md#building-from-source) to enable placeholder decoding in a source build.

Add new targets by extending the matrix in `release.yml`. Pull requests that change release configuration and manual runs build validation archives without publishing a release.

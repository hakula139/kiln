---
name: release
description: Cut a kiln release tag. Use whenever the user asks to release, tag, ship, or cut version X.Y.Z of kiln (e.g., "release v0.3.0-rc.1", "let's tag 0.3.0", "ship the rc"). Wraps the canonical procedure in `RELEASING.md` with reminders about the common pitfalls (cliff `--prepend` vs `--output`, the manual compare-link footer). Use even if the user just says "release X.Y.Z" without further context.
---

# Cut a kiln release

Read `RELEASING.md` at the repo root and execute the procedure step by step. It is the source of truth — do not improvise around it.

## Reminders

These are the steps where mistakes are most likely:

- **Cliff invocation.** Stable releases use `git cliff --unreleased --tag vX.Y.Z --prepend CHANGELOG.md`. Prereleases add `--tag-pattern '^v[0-9]+\.'` so their notes cover only changes since the previous tag. Use `--prepend` to preserve existing sections.
- **Compare-link footer.** `--prepend` does not regenerate the footer block. Add the new line by hand using the previous stable tag for a stable release and the previous tag for a prerelease.
- **Diff sanity check.** Confirm the diff contains only the version and changelog updates described in `RELEASING.md`.
- **Tag confirmation.** Pushing the tag triggers the GitHub release workflow and is hard to undo cleanly. Confirm with the user before `git push origin vX.Y.Z`, even if they already approved the version bump.

After the tag is pushed, watch the workflow with `gh run watch` and report the resulting release URL.

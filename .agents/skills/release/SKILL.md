---
name: release
description: Cut a kiln release tag. Use whenever the user asks to release, tag, ship or cut version X.Y.Z of kiln (e.g., "release v0.3.0-rc.1", "let's tag 0.3.0", "ship the rc"). Follows the canonical procedure in `RELEASING.md`. Use even if the user just says "release X.Y.Z" without further context.
---

# Cut a kiln release

Read `RELEASING.md` at the repo root and execute its procedure step by step, since it is the source of truth for releases.

After the tag is pushed, watch the workflow with `gh run watch` and report the resulting release URL.

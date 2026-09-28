# Desktop prerelease publication

`Publish Desktop Prerelease` is a manual-only workflow for publishing an existing,
successful `Desktop Windows` run from this fork's `main`. It never builds new
product binaries or publishes a stable version. Dispatch it only when the user
has requested that prerelease; publishing an installer remains a separate gate.

Add reviewed notes named `v<version>.md` here, then dispatch the workflow with the
successful source `run_id`. The first Markdown heading is used as the release
title. The version and tag come from the source commit's Tauri configuration.

The workflow verifies the source run, portable checksum, `COMMIT.txt`, EXE
version, required notices/docs and absence of a bundled WebView2 runtime. It tags
the original product commit as `Khun <khun.mail@qq.com>`, uploads a draft, checks
the server-side asset digest, and publishes it as a prerelease. A matching draft
can be resumed; an existing tag pointing elsewhere or a published release is
never overwritten. Stable versions are rejected.

This dedicated publisher does not re-enable inherited upstream publishers or
external-service workflows. Keep it manual-only when merging upstream.

# Desktop release publication

`Publish Desktop Release` (`desktop-prerelease.yml`, retained for compatibility)
is manual-only and publishes an existing successful `Desktop Windows` run from
this fork's `main`. Dispatch only for a user-requested release. The channel
defaults to `prerelease`; select `stable` only when explicitly authorized.
Installer publication remains a separate gate.

Add reviewed notes named `v<version>.md` here, then dispatch with the successful
source `run_id` and intended `channel`. The first heading supplies the title.
Version and tag come from the source commit's Tauri configuration. Stable accepts
only major.minor.patch; prerelease accepts alpha/beta/rc versions.

The publisher verifies the source run, portable checksum, COMMIT.txt, EXE version,
required notices/docs and absence of a bundled WebView2 runtime. It tags the
source as Khun <khun.mail@qq.com>, uploads a draft, verifies the server asset
digest, then publishes. Stable is marked Latest; prereleases are not. A matching
draft can resume; published releases and conflicting tags are never overwritten.

Desktop Windows runs full checks by default for pushes, PRs and manual runs.
For an explicitly requested version-only promotion of an already validated
release, manual `validation=release-smoke` rebuilds UI/gateway/host, launches the
extracted portable package and checks lifecycle/readiness. It skips repeated UI,
Rust integration and Settings suites. Record the validated baseline and skipped
coverage in release notes; do not use this mode for functional changes.

Keep this publisher manual-only. It does not re-enable inherited upstream
publishers or external-service workflows.

# Portable alpha validation

## Custom listen address (alpha.3)

The current suite contains fourteen unit/helper tests and two real-sidecar
integration tests. Shared validation fixtures cover IPv4, IPv6 and localhost,
including malformed IPs, URLs, ports, DNS names, IPv6 brackets/zones and empty
input. Rust validates independently before stopping the existing gateway.

Real-sidecar tests exercise custom loopback addresses, IPv6 loopback/wildcard,
localhost, restart persistence, unassigned-address rollback and listener ownership.
The native WebView CI test uses the same validation fixtures, confirms invalid
input disables Save, then exercises localhost, a different loopback IP, IPv6 and
an assigned runner NIC address. Health/API checks, the original dashboard and
persisted restart must follow the actual address. The portable release host is
also tested after extracting the final archive. No upstream UI or gateway source
changes are required. Release notes record the final CI result.

The alpha.2 and alpha.1 acceptance records below are historical.

## Desktop Settings (alpha.2)

The settings implementation has twelve unit/helper tests in total and two
real-sidecar integration tests. Local Windows runs passed settings migration,
validation, IPC origin checks, port changes, occupied-port rollback, automatic
fallback with a preserved preference, process restart, state-write-failure
rollback, explicit LAN binding, return to loopback, cancellation, and key reuse.
The Windows wildcard-binding regression check rejects a port occupied on a
specific local address even if a wildcard bind would otherwise succeed. Readiness
also rejects overlapping listeners owned by another process.

The Windows workflow additionally runs `scripts/settings-smoke.cjs` against the
actual debug host/WebView in a disposable CI profile. It checks the separate
settings window, remote UI IPC denial, invalid input, rollback, fallback,
persistence across host restart, LAN confirmation, loopback navigation, independent
window close and light/dark/minimum-size screenshots. Screenshots are uploaded as
`desktop-settings-test-evidence`; release notes record the observed result.
The published portable binary is a release build; test-only window launch support
and Playwright are not included as runtime dependencies.

The original alpha.1 desktop acceptance record follows.

The first release uses the installed Microsoft WebView2 runtime. No installer
or bundled-runtime release is included in the first acceptance gate.

## Windows desktop checks

On Windows 10 x64 (build 19044), with the installed WebView2 runtime, the release
host was launched without a terminal. The original Bifrost dashboard, Provider
management, provider key form and Client Settings were exercised in its actual
Tauri WebView. A setting was saved, survived restart, and was restored afterward.
A provider with no paid credentials was sufficient for these checks.

Native tray automation (`scripts/tray-smoke.py`, optional development dependency
`pywinauto==0.6.9`) verified all of the following against the running desktop:

- Closing the main window hides it while `/health` continues returning 200.
- **Show bif-app** restores the window.
- **Copy API Base URL** copies the actual selected `/v1` endpoint.
- **Start at Login** registers and unregisters correctly; its initial off state
  is restored after the test.
- Opening the UI in the default browser preserves the application window.
- **Quit** exits both host and sidecar. Gateway logs confirm storage cleanup
  and normal server shutdown.

A second launch exited successfully without replacing the sidecar. Forced host
termination reaped the gateway through its Windows Job Object. Restart reused
the existing database, selected port and Credential Manager key. The gateway's
listening socket was verified as `127.0.0.1`, with no LAN listener.

The tray script performs real mouse/menu operations and should only be run when
the desktop is available for testing. It restores clipboard text and autostart
state. Example (replace the process IDs and actual port):

```powershell
python desktop/scripts/tray-smoke.py --pid 1234 --child-pid 5678 --port 8080
```

## Repeatable automated checks

`desktop/scripts/build.ps1 test` runs fourteen desktop tests covering paths, command
construction, port conflicts and persistence, HTTP readiness and failure cases,
key creation/reuse/failure handling, and actual Windows Credential Manager
round trips. Two additional real-sidecar integration tests verify:

- HTTP health, embedded original UI/assets and management endpoints;
- `/v1/models` and an OpenAI-compatible `/v1/chat/completions` request through
  the original gateway to a local mock provider (no paid provider credentials);
- graceful stdin-triggered shutdown, cleanup and persisted provider data;
- restart with the same port/key, and crash reaping;
- absence of the generated encryption key in configuration, database and logs.

The **Desktop Windows** workflow builds from a clean Windows Server 2022 runner,
tests the gateway, extracts the final portable ZIP and launches its actual host.
`scripts/host-smoke.ps1` checks the GUI PE subsystem, original UI, owned loopback
listener, single-instance behavior, crash reaping and restart. It deliberately
requires an empty disposable CI profile. The workflow also builds the ordinary
gateway entry point to catch server-mode regressions.

For alpha.4, `scripts/oss-ui-smoke.mjs` also builds and tests both ordinary and
desktop production UI bundles against a disposable gateway (five browser cases
per mode). It verifies the complete audited navigation list, search/keyboard
navigation, connector selectors, API Keys auth guidance, direct placeholder
routes, feature-flag rows, and real temporary prompt creation/editing/deletion.
Ordinary builds retain Enterprise placeholder entries; desktop builds hide them.
See [OSS_UI_PLAN.md](OSS_UI_PLAN.md) for the exact presentation-only scope.

GitHub-hosted CI does not automate interactive tray menus; the Windows 10 desktop
checks above supply that coverage. A physical Windows 11 session, real paid LLM
calls, and the optional fixed-WebView2 archive have not been validated. Pi Agent
and OpenCode were not individually launched; compatibility is checked at their
standard OpenAI-compatible HTTP interface. Release assets are unsigned.

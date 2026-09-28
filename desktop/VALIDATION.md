# Portable alpha validation

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

`desktop/scripts/build.ps1 test` runs eight desktop tests covering paths, command
construction, port conflicts and persistence, HTTP readiness and failure cases,
key creation/reuse/failure handling, and actual Windows Credential Manager
round trips. An additional real-sidecar integration test verifies:

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

GitHub-hosted CI does not automate interactive tray menus; the Windows 10 desktop
checks above supply that coverage. A physical Windows 11 session, real paid LLM
calls, and the optional fixed-WebView2 archive have not been validated. Pi Agent
and OpenCode were not individually launched; compatibility is checked at their
standard OpenAI-compatible HTTP interface. Release assets are unsigned.

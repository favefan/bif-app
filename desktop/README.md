# bif-app for Windows

bif-app is an independent desktop wrapper based on the open-source Bifrost AI
Gateway by Maxim.

Bifrost is developed by Maxim.
bif-app is not affiliated with or endorsed by Maxim.

The original Bifrost UI and HTTP gateway run inside a thin Tauri v2 Windows host.
Providers, routing, plugins, logging, usage, MCP and the model catalog retain
their original behavior. The desktop UI hides audited Enterprise-only placeholder
entries and upgrade prompts; see [the presentation scope and maintenance plan](OSS_UI_PLAN.md).
This is not a new gateway or a second configuration system.

## Portable usage

Download `bif-app-windows-x64-portable.zip` and `SHA256SUMS.txt` from the GitHub
pre-release or the **Desktop Windows** Actions artifact. Extract the entire ZIP
to a writable local folder, then double-click **bif-app.exe**. Keep its sidecar
with the application. This standard, smaller ZIP uses the Microsoft WebView2
Evergreen runtime already installed on your PC. An optional
`bif-app-windows-x64-portable-with-webview2.zip` can carry the runtime for offline
machines; keep its `WebView2` folder together with the executables. No terminal,
Docker, development tools, cloud account or separately installed Bifrost is
needed. Windows 10/11 x86_64 only.
The alpha is unsigned; no certificate or installer is required for this phase.

Wait for startup to finish: the window opens the original Bifrost UI after
`GET /health` returns HTTP 200. Provider credentials are configured in that UI.
Basic startup and API validation do not require a paid provider.

The default API base URL is **http://127.0.0.1:8080/v1**. If occupied, bif-app
selects a free port in 8080–8180 and reuses it on the next launch. It probes an
occupied port but never attaches to or kills an unowned server, even one
responding to Bifrost health checks. **Copy API Base URL** in the tray gives the
actual endpoint for an OpenAI-compatible client such as Pi Agent or OpenCode.
Use Bifrost's normal provider/model naming. No client configuration is modified.

## Desktop Settings

Choose **Desktop Settings…** from the system tray to open a small, separate
desktop window. This network configuration lives in the desktop layer. The small
[Enterprise presentation changes](OSS_UI_PLAN.md) in the original UI do not
change routes, permissions, APIs, or business configuration.

- **监听地址 / Listen address**: a text field with default value and placeholder
  `127.0.0.1`. Enter an IPv4 address, an IPv6 literal (without URL brackets or a
  zone identifier), or `localhost`. Invalid input shows an inline error and
  disables Save; the Rust host independently validates before stopping anything.
  `localhost` always binds to `127.0.0.1`, without DNS resolution. An address that
  cannot be bound on this computer fails clearly and triggers rollback.
- **首选端口 / Preferred port**: any port from 1 to 65535. The optional automatic
  fallback tries the following 100 ports, wrapping after 65535. Turn it off to
  require the exact port. Occupied services are never killed or reused.
- **Current API / Actual listener**: reports the active endpoint separately from
  your saved preference. Automatic fallback does not overwrite your preferred port.
- **保存并重启 Gateway / Save and restart Gateway**: gracefully stops the owned
  gateway, starts it with the new settings and checks its health and socket owner.
  API requests are briefly interrupted. Failed startup or state persistence triggers
  an attempt to restore the previous settings, with a clear error if recovery fails.

The main window, health checks and tray API URL follow the actual address and
port. Wildcard `0.0.0.0` uses `127.0.0.1` for desktop access; `::` uses `::1`.
IPv6 API URLs include brackets. When using a non-loopback address, configure
appropriate authentication in the original Bifrost UI; other devices may reach
both the API and management UI. No firewall rule is added automatically.
Closing Desktop Settings closes only that window. Only the bundled settings
window has permission to read/apply desktop settings; the Bifrost WebView does not.

Closing the window hides it to the system tray and keeps the gateway available.
Click the tray icon or choose **Show bif-app** to restore it. **Quit** closes
the sidecar's private stdin pipe, waits for Bifrost's normal cleanup, and uses
owned-process termination only after a timeout. Windows Job Objects also
terminate the owned gateway and its descendants if the desktop process crashes.
A second launch shows the existing window and does not start another gateway.

**Start at Login** defaults to off and is controlled through the tray. Windows'
per-user autostart registration is the source of truth for that desktop setting.
If you move the portable folder, toggle this setting off and on from its new
location. External web links open in your default browser.

## Data, credentials and logs

- `%LOCALAPPDATA%\bif-app\bifrost\`: original Bifrost local SQLite databases,
  configuration and logs. This is the only business configuration store.
- `%LOCALAPPDATA%\bif-app\desktop\state.json`: desktop listen address, preferred
  port, automatic fallback choice and last selected port. Alpha.1 port-only state
  is migrated automatically, preserving its port preference and loopback default.
- `%LOCALAPPDATA%\bif-app\desktop\gateway.log`: stdout/stderr, with previous
  startup log rotation above 5 MiB; startup errors also appear in a native dialog.
- `%LOCALAPPDATA%\bif-app\desktop\webview\`: WebView browser profile.
- Windows Credential Manager, generic credential
  `bif-app/gateway-encryption-key/v1`: a securely generated 256-bit random key.

The key is passed only in the child environment as `BIFROST_ENCRYPTION_KEY`,
never in arguments, JSON, SQLite or desktop logs. Credential failures stop
startup. If data exists but the credential is missing, restore the original
credential: the app will not generate a replacement that makes old data unreadable.
Back up the Bifrost directory together with the credential using an appropriate
secure Windows backup. Deleting the ZIP does not delete data or the credential.
Existing standalone Bifrost installations and `~/.config/bifrost` are not used.

Desktop adds no telemetry, updater, analytics, account service or reverse proxy.
Existing upstream external integrations remain controlled by Bifrost.

## Develop and build

Build prerequisites: Windows x64, the Go version declared by `transports/go.mod`,
Node from root `.nvmrc`, Rust stable MSVC with Visual Studio C++ build tools and
Windows SDK, and MinGW-w64 GCC (or LLVM-MinGW clang) for CGO SQLite. Set `CC` if
the CGO compiler is not on PATH. Rust dependency versions are exact and locked.
Go and npm dependencies use the existing repository lock/sum files.

From a clean checkout in PowerShell:

```powershell
./desktop/scripts/build.ps1 sidecar
./desktop/scripts/build.ps1 debug
./desktop/scripts/build.ps1 test
./desktop/scripts/build.ps1 release
```

The sidecar target builds the original Vite UI, typechecks it, copies the
generated output to the existing Go embed directory, then builds the original
HTTP entry point with CGO SQLite. A generated local `go.work` resolves core,
framework, transport and every plugin from this checkout. Never use `GOWORK=off`
for desktop release builds. Release creates the portable ZIP and SHA256 in
`desktop/dist/`. `COMMIT.txt` identifies the source commit for host and gateway.
`package` repackages an already-built release; it does not compile stale sources.

To also package an official Microsoft fixed WebView2 runtime, use
`./desktop/scripts/build.ps1 package -IncludeWebView2`. The default ZIP uses the
installed runtime and omits that download. The optional runtime's
download version and checksum are pinned in `scripts/webview.ps1`; update them
together after verification. This runtime is serviced by publishing a new ZIP,
not by a desktop network updater.

## Validation

`build.ps1 test` runs Rust unit tests (port conflicts/persistence, local paths,
command construction, HTTP health, key creation/reuse and actual Windows
Credential Manager), then real-sidecar smoke tests. No paid API is called.
The settings smoke test also covers port changes, occupied-port and write-failure
rollback, persisted preference versus actual port, custom IPv4/IPv6/localhost,
unassigned-address rollback and cancellation.
`scripts/settings-smoke.cjs` runs in a disposable Windows CI profile against a
debug host and its actual WebView. The debug-only `--test-settings-window` flag
opens the same window as the tray entry; release builds do not expose this flag.
Its Playwright dependency is confined to `desktop/.tools/qa`, outside the product.
Windows CI performs builds and validation before uploading an artifact.
GUI checks and any platform coverage limitations are recorded in the release
notes; a successful backend smoke test alone does not prove tray usability.
See [VALIDATION.md](VALIDATION.md) for the exact checks and native tray test.

## Upstream maintenance

`main` is bif-app's default and integration branch. `origin` points to this fork;
`upstream` points to `maximhq/bifrost`, whose integration branch is `dev`.
The fork does not maintain a separate `origin/dev` mirror. Start feature/fix
branches from `main` and merge them back into `main`.

The fork's GitHub Actions settings disable the inherited **Release Pipeline**,
**Release CLI** and **Release Migration CLI** workflows: their `main` triggers
publish upstream products, not bif-app. Keep these disabled when maintaining
this fork. **Desktop Windows** remains the desktop build/validation workflow.
The upstream CodSpeed job is restricted to `maximhq/bifrost` because its managed
runner is unavailable to this fork. Other upstream checks are retained.

The fork also disables these inherited workflows in repository Actions settings
after inspecting their failed runs (2026-09-28):

- **Snyk checks** fails in `step-security/setup-uv` with an invalid-subscription
  error, before scanning. Its external service prerequisites are not configured
  for bif-app; this is not a successful security scan or a vulnerability finding.
- **Scorecard supply-chain security** fails pulling the external
  `gcr.io/openssf/scorecard-action:v2.4.0` image with a billing-required error.
  No Scorecard assessment completes.
- **PR Test Notifier** runs on `push` but expects a pull-request number, so its
  comment command fails. It is unrelated to desktop validation.

These workflow files are retained to minimize upstream differences. Do not
re-enable them just when syncing upstream; first repair/verify their prerequisites
and suitability for this fork. Desktop builds and tests, workflow lint and
dependency review remain enabled. GitHub Actions repository settings are separate
from versioned workflow files, so a fresh fork must apply this policy explicitly.

The root [`AGENTS.md`](../AGENTS.md) makes these rules mandatory for future agents:

1. Keep every necessary upstream source change minimal and in its own commit,
   explaining the reason, preservation of ordinary server behavior, and tests.
2. Merge upstream through Git, resolve source conflicts, then validate the
   desktop release, desktop tests/smoke checks and ordinary server build. Do not
   add a build-time patch application layer.
3. When upstream provides an equivalent integration capability, use it and
   remove the redundant fork modification after checking equivalent behavior.

Published commits and release tags are not rewritten just to reorganize history.

```powershell
git remote add upstream https://github.com/maximhq/bifrost.git
git switch main
git fetch upstream
git merge upstream/dev
./desktop/scripts/build.ps1 release
./desktop/scripts/build.ps1 test
go build -o desktop/dist/bifrost-server-check.exe ./transports/bifrost-http
git push origin main
```

Add the remote only when it does not already exist. Keep desktop changes in
`desktop/` and the dedicated workflow. The small, marked upstream change is an
optional `-shutdown-on-stdin-close` argument and `StartWithShutdown` method;
ordinary server launches still follow their original OS-signal lifecycle.
The API, UI and business configuration are not forked inside this repository.

The upstream Apache-2.0 LICENSE and notices must remain with redistributions.
See `THIRD_PARTY_NOTICES.md` here for desktop/runtime attribution. Phase one is
portable only; an NSIS installer is intentionally deferred until user acceptance.

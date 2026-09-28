# bif-app for Windows

bif-app is an independent desktop wrapper based on the open-source Bifrost AI
Gateway by Maxim.

Bifrost is developed by Maxim.
bif-app is not affiliated with or endorsed by Maxim.

The original Bifrost UI, HTTP gateway, providers, routing, plugins, logging,
usage, MCP and model catalog run unchanged inside a thin Tauri v2 Windows host.
This is not a new gateway or a second configuration system.

## Portable usage

Download `bif-app-windows-x64-portable.zip` and `SHA256SUMS.txt` from the GitHub
pre-release or the **Desktop Windows** Actions artifact. Extract the entire ZIP
to a writable local folder, then double-click **bif-app.exe**. Keep its sidecar
and `WebView2` folder together. No terminal, Docker, development tools, cloud
account or separately installed Bifrost is needed. Windows 10/11 x86_64 only.
The alpha is unsigned; no certificate or installer is required for this phase.

Wait for startup to finish: the window opens the original Bifrost UI after
`GET /health` returns HTTP 200. Provider credentials are configured in that UI.
Basic startup and API validation do not require a paid provider.

The preferred API base URL is **http://127.0.0.1:8080/v1**. If occupied, bif-app
selects a free port in 8080–8180 and reuses it on the next launch. It probes an
occupied port but never attaches to or kills an unowned server, even one
responding to Bifrost health checks. **Copy API Base URL** in the tray gives the
actual endpoint for an OpenAI-compatible client such as Pi Agent or OpenCode.
Use Bifrost's normal provider/model naming. No client configuration is modified.

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
- `%LOCALAPPDATA%\bif-app\desktop\state.json`: selected port only.
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

The portable ZIP carries an official Microsoft fixed WebView2 runtime. Its
download version and checksum are pinned in `scripts/webview.ps1`; update them
together after verification. This runtime is serviced by publishing a new ZIP,
not by a desktop network updater.

## Validation

`build.ps1 test` runs Rust unit tests (port conflicts/persistence, local paths,
command construction, HTTP health, key creation/reuse and actual Windows
Credential Manager), then real-sidecar smoke tests. No paid API is called.
Windows CI performs builds and validation before uploading an artifact.
GUI checks and any platform coverage limitations are recorded in the release
notes; a successful backend smoke test alone does not prove tray usability.

## Upstream maintenance

```powershell
git remote add upstream https://github.com/maximhq/bifrost.git
git fetch upstream
git merge upstream/dev
./desktop/scripts/build.ps1 release
./desktop/scripts/build.ps1 test
```

Add the remote only when it does not already exist. Keep desktop changes in
`desktop/` and the dedicated workflow. The small, marked upstream change is an
optional `-shutdown-on-stdin-close` argument and `StartWithShutdown` method;
ordinary server launches still follow their original OS-signal lifecycle.
The API, UI and business configuration are not forked inside this repository.

The upstream Apache-2.0 LICENSE and notices must remain with redistributions.
See `THIRD_PARTY_NOTICES.md` here for desktop/runtime attribution. Phase one is
portable only; an NSIS installer is intentionally deferred until user acceptance.

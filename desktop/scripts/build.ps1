param([ValidateSet('sidecar','debug','release','test','package')][string]$Target = 'release', [switch]$IncludeWebView2)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$desktop = Join-Path $repo 'desktop'
function Run([string]$Exe, [string[]]$Arguments) {
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Exe failed with exit code $LASTEXITCODE" }
}
Push-Location $repo
try {
    # These paths are development conveniences, not runtime dependencies.
    $localGo = Join-Path $desktop '.tools/go/bin'
    if (Test-Path $localGo) { $env:PATH = "$localGo;$env:PATH" }
    $localClang = Get-ChildItem "$desktop/.tools" -Directory -Filter 'llvm-mingw-*-ucrt-x86_64' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($localClang -and -not $env:CC) { $env:CC = Join-Path $localClang.FullName 'bin/clang.exe' }
    if ($Target -in @('sidecar','debug','release')) {
        Run npm @('ci','--prefix','ui','--no-audit','--no-fund')
        # Invoke the very same Vite and typecheck commands; upstream copy-build
        # uses POSIX rm/cp, so copy the generated output natively on Windows.
        Push-Location ui
        $previousDesktopHideEnterpriseUI = [Environment]::GetEnvironmentVariable('BIFROST_DESKTOP_HIDE_ENTERPRISE_UI', 'Process')
        try {
            # Desktop-only UI presentation flag; do not persist it into gateway/business configuration.
            $env:BIFROST_DESKTOP_HIDE_ENTERPRISE_UI = 'true'
            Run npx @('--no-install','vite','build')
            Run npm @('run','typecheck')
        } finally {
            if ($null -eq $previousDesktopHideEnterpriseUI) {
                Remove-Item Env:BIFROST_DESKTOP_HIDE_ENTERPRISE_UI -ErrorAction SilentlyContinue
            } else {
                $env:BIFROST_DESKTOP_HIDE_ENTERPRISE_UI = $previousDesktopHideEnterpriseUI
            }
            Pop-Location
        }
        $embedded = Join-Path $repo 'transports/bifrost-http/ui'
        if (Test-Path $embedded) {
            if ((Resolve-Path $embedded).Path -ne "$repo\transports\bifrost-http\ui") { throw 'Unexpected UI output path' }
            Remove-Item -LiteralPath $embedded -Recurse -Force
        }
        Copy-Item -LiteralPath "$repo/ui/out" -Destination $embedded -Recurse
        # All local modules, never released module copies: one source commit.
        if (-not (Test-Path go.work)) { Run go @('work','init','./core','./framework','./transports') }
        Run go @('work','use','./core','./framework','./transports')
        foreach ($module in Get-ChildItem plugins -Directory) {
            if (Test-Path (Join-Path $module.FullName 'go.mod')) { Run go @('work','use',$module.FullName) }
        }
        $env:CGO_ENABLED = '1'; $env:GOOS = 'windows'; $env:GOARCH = 'amd64'
        $commit = (git rev-parse HEAD).Trim()
        New-Item -ItemType Directory -Force "$desktop/src-tauri/binaries" | Out-Null
        Run go @('build','-trimpath','-tags','sqlite_static','-ldflags',"-s -w -extldflags=-static -X main.Version=bif-app-$($commit.Substring(0,12))",'-o',"$desktop/src-tauri/binaries/bifrost-http.exe",'./transports/bifrost-http')
        Set-Content "$desktop/src-tauri/binaries/COMMIT.txt" $commit
    }
    if ($Target -eq 'sidecar') { return }
    if ($Target -eq 'test') {
        Run cargo @('test','--locked','--manifest-path',"$desktop/src-tauri/Cargo.toml",'--','--test-threads=1')
        $env:BIF_APP_TEST_SIDECAR = "$desktop/src-tauri/binaries/bifrost-http.exe"
        Run cargo @('test','--locked','--manifest-path',"$desktop/src-tauri/Cargo.toml",'--','--ignored','--test-threads=1')
        return
    }
    if ($Target -in @('debug','release')) {
        $cargoArgs = @('build','--locked','--manifest-path',"$desktop/src-tauri/Cargo.toml")
        if ($Target -eq 'release') { $cargoArgs += '--release' }
        Run cargo $cargoArgs
        Copy-Item "$desktop/src-tauri/binaries/bifrost-http.exe" "$desktop/src-tauri/target/$Target/bifrost-http.exe" -Force
        Copy-Item "$desktop/src-tauri/binaries/COMMIT.txt" "$desktop/src-tauri/target/$Target/COMMIT.txt" -Force
        if ($Target -eq 'debug') { return }
    }
    & "$PSScriptRoot/package.ps1" -IncludeWebView2:$IncludeWebView2
} finally { Pop-Location }

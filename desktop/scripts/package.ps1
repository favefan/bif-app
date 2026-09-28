param([switch]$IncludeWebView2)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$desktop = Join-Path $repo 'desktop'
$currentCommit = (git -C $repo rev-parse HEAD).Trim()
$gatewayCommit = (Get-Content "$desktop/src-tauri/binaries/COMMIT.txt" -Raw).Trim()
$hostCommit = (Get-Content "$desktop/src-tauri/target/release/COMMIT.txt" -Raw).Trim()
if ($currentCommit -ne $gatewayCommit -or $hostCommit -ne $gatewayCommit) { throw 'Host, gateway and checkout must share one commit. Run build.ps1 release.' }
$name = if ($IncludeWebView2) { 'bif-app-windows-x64-portable-with-webview2' } else { 'bif-app-windows-x64-portable' }
$stage = Join-Path $desktop "dist/$name"
if (Test-Path $stage) {
    $expectedStage = [IO.Path]::GetFullPath((Join-Path $desktop "dist/$name"))
    if ((Resolve-Path $stage).Path -ne $expectedStage) { throw 'Unexpected package staging path' }
    Remove-Item -LiteralPath $stage -Recurse -Force
}
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "$desktop/src-tauri/target/release/bif-app.exe", "$desktop/src-tauri/binaries/bifrost-http.exe", "$desktop/src-tauri/binaries/COMMIT.txt", "$repo/LICENSE", "$repo/THIRD_PARTY_NOTICES.md" -Destination $stage -Force
Copy-Item "$desktop/README.md" "$stage/README.md" -Force
Copy-Item "$desktop/VALIDATION.md" "$stage/VALIDATION.md" -Force
Copy-Item "$desktop/THIRD_PARTY_NOTICES.md" "$stage/DESKTOP_THIRD_PARTY_NOTICES.md" -Force
node "$PSScriptRoot/licenses.mjs" $stage
if ($LASTEXITCODE -ne 0) { throw 'Dependency license collection failed' }
if ($IncludeWebView2) { & "$PSScriptRoot/webview.ps1" -Destination "$stage/WebView2" }
$zip = Join-Path $desktop "dist/$name.zip"
Compress-Archive -Path "$stage/*" -DestinationPath $zip -Force -CompressionLevel Optimal
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content -Encoding ascii "$desktop/dist/SHA256SUMS.txt" "$hash  $name.zip"
Write-Output "Portable: $zip"
Write-Output "SHA256: $hash"

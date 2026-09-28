$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$desktop = Join-Path $repo 'desktop'
$stage = Join-Path $desktop 'dist/bif-app-windows-x64-portable'
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "$desktop/src-tauri/target/release/bif-app.exe", "$desktop/src-tauri/binaries/bifrost-http.exe", "$desktop/src-tauri/binaries/COMMIT.txt", "$repo/LICENSE", "$repo/THIRD_PARTY_NOTICES.md" -Destination $stage -Force
Copy-Item "$desktop/README.md" "$stage/README.md" -Force
Copy-Item "$desktop/THIRD_PARTY_NOTICES.md" "$stage/DESKTOP_THIRD_PARTY_NOTICES.md" -Force
& "$PSScriptRoot/webview.ps1" -Destination "$stage/WebView2"
if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) { throw 'WebView2 preparation failed' }
$zip = Join-Path $desktop 'dist/bif-app-windows-x64-portable.zip'
Compress-Archive -Path "$stage/*" -DestinationPath $zip -Force -CompressionLevel Optimal
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content -Encoding ascii "$desktop/dist/SHA256SUMS.txt" "$hash  bif-app-windows-x64-portable.zip"
Write-Output "Portable: $zip"
Write-Output "SHA256: $hash"

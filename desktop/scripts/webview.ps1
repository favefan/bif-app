param([Parameter(Mandatory)][string]$Destination)
$ErrorActionPreference = 'Stop'
$desktop = (Resolve-Path "$PSScriptRoot/..").Path
$cache = Join-Path $desktop '.tools'
New-Item -ItemType Directory -Force $cache,$Destination | Out-Null
# Official Microsoft fixed runtime, pinned after SHA256 verification.
$url = 'https://msedge.sf.dl.delivery.mp.microsoft.com/filestreamingservice/files/b82d47e8-d146-4563-94d1-3a3176b25c0a/Microsoft.WebView2.FixedVersionRuntime.154.0.4258.37.x64.cab'
$sha256 = '143da7f7c4939fddd3875ed918e44022d7eb87063bf912fe3e32df37c6b0b8c3'
$cab = Join-Path $cache 'webview2.cab'
if (-not (Test-Path $cab)) { Invoke-WebRequest $url -OutFile $cab }
if ((Get-FileHash $cab -Algorithm SHA256).Hash.ToLowerInvariant() -ne $sha256) { throw 'WebView2 checksum mismatch' }
if (-not (Test-Path "$Destination/msedgewebview2.exe")) {
    $expanded = Join-Path $cache 'webview-expanded'
    New-Item -ItemType Directory -Force $expanded | Out-Null
    & expand.exe $cab '-F:*' $expanded | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'WebView2 extraction failed' }
    $binary = Get-ChildItem $expanded -Recurse -Filter msedgewebview2.exe | Select-Object -First 1
    if (-not $binary) { throw 'Missing WebView2 binary' }
    Copy-Item "$($binary.Directory.FullName)/*" $Destination -Recurse -Force
}
# Microsoft requires these grants for unpackaged fixed runtimes on Windows 10.
# They grant read/execute to AppContainers only, never write or network access.
& icacls.exe $Destination /grant '*S-1-15-2-1:(OI)(CI)(RX)' /grant '*S-1-15-2-2:(OI)(CI)(RX)' /T /Q | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'WebView2 runtime permissions failed' }

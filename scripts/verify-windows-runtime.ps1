param(
  [Parameter(Mandatory = $true)]
  [string]$ReleaseDir
)

$ErrorActionPreference = "Stop"
$release = (Resolve-Path -LiteralPath $ReleaseDir).Path
$app = Join-Path $release "foxcull.exe"
$ffmpeg = Join-Path $release "ffmpeg.exe"

if (-not (Test-Path -LiteralPath $app -PathType Leaf)) {
  throw "Windows runtime check: missing $app"
}
if (-not (Test-Path -LiteralPath $ffmpeg -PathType Leaf)) {
  throw "Windows runtime check: missing required ffmpeg sidecar at $ffmpeg"
}

# WebView2Loader is linked statically by the supported Windows-MSVC release
# build. Windows-GNU links it dynamically; Tauri copied the DLL beside the raw
# release executable but omitted it from the NSIS installer on 2026-07-24. A
# directory check therefore gave false confidence. Reject GNU distribution
# categorically instead of trying to mirror Tauri's private bundling behavior.
$rustHost = (& rustc -vV | Where-Object { $_ -like "host:*" }) -replace "^host:\s*", ""
if ($rustHost -like "*windows-gnu*") {
  throw @"
Windows runtime check: Windows-GNU artifacts are not distributable. Tauri's
installer omitted the dynamically required WebView2Loader.dll even when it was
present beside the raw executable. Build through GitHub Actions
(Windows-MSVC), which is FoxCull's supported Windows release path.
"@
}

# Catch missing DLLs and other immediate startup failures on the same runner
# that produced the artifact. A healthy app stays open; close it after the
# smoke window so release packaging can continue.
$process = $null
try {
  $process = Start-Process -FilePath $app -PassThru
  Start-Sleep -Seconds 8
  $process.Refresh()
  if ($process.HasExited) {
    throw "Windows runtime check: foxcull.exe exited during startup (code $($process.ExitCode))"
  }
  if ($process.MainWindowTitle -like "*System Error*") {
    throw "Windows runtime check: foxcull.exe opened a system-error dialog: $($process.MainWindowTitle)"
  }
  Write-Host "Windows runtime check passed: app launched and ffmpeg sidecar is present."
}
finally {
  if ($null -ne $process) {
    $process.Refresh()
    if (-not $process.HasExited) {
      Stop-Process -Id $process.Id -Force
    }
  }
}

param(
  [string]$Source = "",
  [string]$DestDir = ""
)

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if ([string]::IsNullOrWhiteSpace($Source)) {
  $Source = Join-Path $Root "release\cli\wiki.exe"
}
if ([string]::IsNullOrWhiteSpace($DestDir)) {
  $DestDir = Join-Path $env:LOCALAPPDATA "llm-wiki\bin"
}

if (!(Test-Path $Source)) {
  $Fallback = Join-Path $Root "target\release\wiki.exe"
  if (Test-Path $Fallback) {
    $Source = $Fallback
  } else {
    Write-Error "CLI binary not found. Expected $Source"
    exit 1
  }
}

New-Item -ItemType Directory -Force -Path $DestDir | Out-Null
Copy-Item -Force $Source (Join-Path $DestDir "wiki.exe")
Write-Host "Installed wiki CLI to $DestDir\wiki.exe"
Write-Host "Add $DestDir to PATH if it is not already there."

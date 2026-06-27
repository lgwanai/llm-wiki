# llm-wiki CLI Installer (Windows PowerShell)
#
# Installs the compiled wiki.exe binary to a directory on PATH.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/install-cli.ps1
#   powershell -ExecutionPolicy Bypass -File scripts/install-cli.ps1 -Source "release\windows-x64\cli\wiki.exe"
#   powershell -ExecutionPolicy Bypass -File scripts/install-cli.ps1 -DestDir "C:\tools"
# ---------------------------------------------------------------------------

param(
  [string]$Source = "",
  [string]$DestDir = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

# ── Resolve source ───────────────────────────────────────────────────────
if ([string]::IsNullOrWhiteSpace($Source)) {
  # Try release directory first
  $Candidates = @(
    (Join-Path $Root "release\windows-x64\cli\wiki.exe"),
    (Join-Path $Root "release\cli\wiki.exe"),
    (Join-Path $Root "target\release\wiki.exe")
  )
  foreach ($c in $Candidates) {
    if (Test-Path $c) {
      $Source = $c
      break
    }
  }
}

if ([string]::IsNullOrWhiteSpace($Source) -or !(Test-Path $Source)) {
  Write-Host "ERROR: CLI binary not found." -ForegroundColor Red
  Write-Host ""
  Write-Host "Build the CLI first:" -ForegroundColor Yellow
  Write-Host "  cargo build --release -p llm-wiki-cli"
  Write-Host "  # or"
  Write-Host "  bash scripts/build-cli.sh --target windows-x64"
  Write-Host ""
  Write-Host "Or pass the binary path:" -ForegroundColor Yellow
  Write-Host "  powershell -File scripts/install-cli.ps1 -Source C:\path\to\wiki.exe"
  exit 1
}

if ([string]::IsNullOrWhiteSpace($DestDir)) {
  $DestDir = Join-Path $env:LOCALAPPDATA "llm-wiki\bin"
}

# ── Show what we found ───────────────────────────────────────────────────
Write-Host "──────────────────────────────────────────────────────────────"
Write-Host "  llm-wiki CLI Installer"
Write-Host "──────────────────────────────────────────────────────────────"
$size = (Get-Item $Source).Length
Write-Host "  Binary:      $Source"
Write-Host "  Size:        $([math]::Round($size / 1MB, 2)) MB"
Write-Host "  Destination: $DestDir\wiki.exe"
Write-Host ""

# ── Install ──────────────────────────────────────────────────────────────
New-Item -ItemType Directory -Force -Path $DestDir | Out-Null
Copy-Item -Force $Source (Join-Path $DestDir "wiki.exe")

Write-Host "  Installed wiki CLI to $DestDir\wiki.exe" -ForegroundColor Green

# ── PATH check ───────────────────────────────────────────────────────────
$onPath = $false
$currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($currentPath -like "*$DestDir*") {
  Write-Host "  wiki is on User PATH" -ForegroundColor Green
  $onPath = $true
} else {
  $currentPath = [Environment]::GetEnvironmentVariable("PATH", "Machine")
  if ($currentPath -like "*$DestDir*") {
    Write-Host "  wiki is on System PATH" -ForegroundColor Green
    $onPath = $true
  }
}

if (-not $onPath) {
  Write-Host ""
  Write-Host "  WARNING: $DestDir is not on PATH." -ForegroundColor Yellow
  Write-Host ""
  Write-Host "  Add it to your User PATH:" -ForegroundColor Yellow
  Write-Host "    [Environment]::SetEnvironmentVariable('PATH', "
  Write-Host "      `$env:PATH + ';$DestDir', 'User')"
  Write-Host ""
  Write-Host "  Then restart your terminal, or add it via System Settings:"
  Write-Host "    Settings → System → About → Advanced system settings → Environment Variables"
}

# ── Quick test ───────────────────────────────────────────────────────────
Write-Host ""
Write-Host "  Run 'wiki --help' to verify the installation."
Write-Host ""

# ── Configuration reminder ───────────────────────────────────────────────
$ConfigFile = Join-Path $env:USERPROFILE ".config\llm-wiki\wiki_config.yaml"
if (!(Test-Path $ConfigFile)) {
  Write-Host "  WARNING: No configuration file found at $ConfigFile" -ForegroundColor Yellow
  Write-Host "  Create one from the example:"
  Write-Host "    mkdir $env:USERPROFILE\.config\llm-wiki"
  Write-Host "    copy wiki_config.yaml.example $ConfigFile"
  Write-Host "    # Then edit with your API keys"
  Write-Host ""
}

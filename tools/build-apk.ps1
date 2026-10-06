<#
.SYNOPSIS
  Builds the signed release APK (docs/DECISIONS.md, Phase 5).

.PARAMETER Env
  dev  - talks to the dev project; for testing on Minas's phone.
  prod - talks to the prod project; only for the employer's phone. Never
         install it on a test phone.

.DESCRIPTION
  Universal APK (arm64, armv7, x86, x86_64), signed with the keystore in
  C:\dev\clock-in-keys\ (keystore.properties). Copied to
  target\apk\clock-in-<version>-dev.apk or target\apk\clock-in-<version>.apk.

.EXAMPLE
  .\tools\build-apk.ps1 -Env dev
#>
param(
    [Parameter(Mandatory)][ValidateSet('dev', 'prod')][string]$Env
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$properties = if ($env:CLOCKIN_KEYSTORE_PROPERTIES) { $env:CLOCKIN_KEYSTORE_PROPERTIES } else { 'C:\dev\clock-in-keys\keystore.properties' }
if (-not (Test-Path $properties)) { throw "No signing keystore settings at $properties (see docs/SETUP.md §6)." }
if (-not (Test-Path (Join-Path $root ".env.$Env"))) { throw "Missing .env.$Env" }

$version = (Get-Content (Join-Path $root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
$env:CLOCKIN_ENV = $Env
& (Join-Path $PSScriptRoot 'android-env.ps1') pnpm tauri android build --apk
if ($LASTEXITCODE -ne 0) { throw "The Android build failed ($LASTEXITCODE)." }

$built = Join-Path $root 'src-tauri\gen\android\app\build\outputs\apk\universal\release\app-universal-release.apk'
if (-not (Test-Path $built)) { throw "No signed APK at $built (was the keystore found?)." }
$name = if ($Env -eq 'dev') { "clock-in-$version-dev.apk" } else { "clock-in-$version.apk" }
$outDir = Join-Path $root 'target\apk'
New-Item -ItemType Directory -Force $outDir | Out-Null
$out = Join-Path $outDir $name
Copy-Item $built $out -Force

# Check the signature (prints the certificate's SHA-256, never a password).
$buildTools = Get-ChildItem (Join-Path $env:ANDROID_HOME 'build-tools') -Directory | Sort-Object { [version]$_.Name } | Select-Object -Last 1
$ErrorActionPreference = 'Continue'
& (Join-Path $buildTools.FullName 'apksigner.bat') verify --print-certs $out | Select-String 'SHA-256'
Write-Host "APK ($Env): $out"

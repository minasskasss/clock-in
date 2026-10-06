<#
.SYNOPSIS
  Builds the signed release APK (docs/DECISIONS.md, Phase 5).

.PARAMETER Env
  dev  - talks to the dev project; for testing on Minas's phone.
  prod - talks to the prod project; only for the employer's phone. Never
         install it on a test phone.

.DESCRIPTION
  One APK for real phones (arm64-v8a and armeabi-v7a), signed with the
  keystore in C:\dev\clock-in-keys\ (keystore.properties). Copied to
  target\apk\clock-in-<version>-dev.apk or target\apk\clock-in-<version>.apk.

  The Rust library is built with cargo and packaged with Gradle directly,
  instead of `pnpm tauri android build`, which needs Windows Developer Mode
  for a symbolic link. The Tauri-generated Gradle glue files must already
  exist (one `pnpm tauri android build` or `android init` creates them).

.EXAMPLE
  .\tools\build-apk.ps1 -Env dev
#>
param(
    [Parameter(Mandatory)][ValidateSet('dev', 'prod')][string]$Env
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
$android = Join-Path $root 'src-tauri\gen\android'

$properties = if ($env:CLOCKIN_KEYSTORE_PROPERTIES) { $env:CLOCKIN_KEYSTORE_PROPERTIES } else { 'C:\dev\clock-in-keys\keystore.properties' }
if (-not (Test-Path $properties)) { throw "No signing keystore settings at $properties (see docs/SETUP.md §6)." }
if (-not (Test-Path (Join-Path $root ".env.$Env"))) { throw "Missing .env.$Env" }
if (-not (Test-Path (Join-Path $android 'app\tauri.build.gradle.kts'))) {
    throw 'Run "pnpm tauri android build" once first (it generates the Gradle glue files).'
}

$version = (Get-Content (Join-Path $root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
$env:CLOCKIN_ENV = $Env
pnpm build
if ($LASTEXITCODE -ne 0) { throw 'Frontend build failed.' }

$abis = [ordered]@{ 'aarch64-linux-android' = 'arm64-v8a'; 'armv7-linux-androideabi' = 'armeabi-v7a' }
foreach ($target in $abis.Keys) {
    & (Join-Path $PSScriptRoot 'android-env.ps1') cargo build --release --package clock-in --manifest-path src-tauri\Cargo.toml --target $target --features tauri/custom-protocol --lib
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $target." }
    $jni = Join-Path $android "app\src\main\jniLibs\$($abis[$target])"
    New-Item -ItemType Directory -Force $jni | Out-Null
    Copy-Item (Join-Path $root "target\$target\release\libclockin_lib.so") (Join-Path $jni 'libclockin_lib.so') -Force
}
# No library for other ABIs may be left over from an earlier build.
Get-ChildItem (Join-Path $android 'app\src\main\jniLibs') -Directory |
    Where-Object { $abis.Values -notcontains $_.Name } |
    ForEach-Object { Remove-Item -LiteralPath $_.FullName -Recurse -Force }

Set-Location $android
$ErrorActionPreference = 'Continue'
.\gradlew.bat assembleUniversalRelease '-PabiList=arm64-v8a,armeabi-v7a' '-ParchList=arm64,arm' '-PtargetList=aarch64,armv7' `
    -x rustBuildUniversalRelease -x rustBuildArm64Release -x rustBuildArmRelease --console=plain
if ($LASTEXITCODE -ne 0) { throw 'Gradle build failed.' }
$ErrorActionPreference = 'Stop'
Set-Location $root

$built = Join-Path $android 'app\build\outputs\apk\universal\release\app-universal-release.apk'
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

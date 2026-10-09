<#
.SYNOPSIS
  Builds the signed release APK (docs/DECISIONS.md, Phase 5).

.PARAMETER Env
  dev  - talks to the dev project; for testing on Minas's phone.
  prod - talks to the prod project; only for the employer's phone. Never
         install it on a test phone.

.PARAMETER Emulator
  dev only: an x86_64 APK for the Android emulator instead
  (target\apk\clock-in-<version>-dev-emulator.apk). Not for phones. Its web
  code is lowered to Chrome 83, the WebView built into the Android 11
  emulator image, which can't be updated there (no Play Store).

.DESCRIPTION
  One APK for real phones (arm64-v8a and armeabi-v7a), signed with the
  keystore in C:\dev\clock-in-keys\ (keystore.properties). Copied to
  target\apk\clock-in-<version>-dev.apk or
  C:\dev\clock-in-releases\<version>-prod\clock-in-<version>.apk. Fails if a
  library embeds the wrong project's URL.

  The Rust library is built with cargo and packaged with Gradle directly,
  instead of `pnpm tauri android build`, which needs Windows Developer Mode
  for a symbolic link. The Tauri-generated Gradle glue files must already
  exist (one `pnpm tauri android build` or `android init` creates them).

.EXAMPLE
  .\tools\build-apk.ps1 -Env dev
  .\tools\build-apk.ps1 -Env dev -Emulator
#>
param(
    [Parameter(Mandatory)][ValidateSet('dev', 'prod')][string]$Env,
    [switch]$Emulator
)
$ErrorActionPreference = 'Stop'
if ($Emulator -and $Env -ne 'dev') { throw 'The emulator build is dev only.' }
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
$android = Join-Path $root 'src-tauri\gen\android'

$properties = 'C:\dev\clock-in-keys\keystore.properties'
if (-not (Test-Path $properties)) { throw "No signing keystore settings at $properties (see docs/SETUP.md §6)." }
if (-not (Test-Path (Join-Path $root ".env.$Env"))) { throw "Missing .env.$Env" }
if (-not (Test-Path (Join-Path $android 'app\tauri.build.gradle.kts'))) {
    throw 'Run "pnpm tauri android build" once first (it generates the Gradle glue files).'
}

$version = (Get-Content (Join-Path $root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
$env:CLOCKIN_ENV = $Env
if ($Emulator) { $env:CLOCKIN_WEB_TARGET = 'chrome83' }
pnpm build
$built = $LASTEXITCODE
Remove-Item Env:CLOCKIN_WEB_TARGET -ErrorAction SilentlyContinue
if ($built -ne 0) { throw 'Frontend build failed.' }

# The project URL each .env file names (public; never printed).
function Get-ProjectUrl([string]$name) {
    $line = Get-Content (Join-Path $root ".env.$name") -ErrorAction SilentlyContinue |
        Where-Object { $_ -match '^\s*SUPABASE_URL\s*=\s*\S' } | Select-Object -First 1
    if ($line) { ($line -split '=', 2)[1].Trim() } else { $null }
}
$wantUrl = Get-ProjectUrl $Env
$otherUrl = Get-ProjectUrl $(if ($Env -eq 'dev') { 'prod' } else { 'dev' })
if (-not $wantUrl) { throw ".env.$Env has no SUPABASE_URL." }
$latin1 = [Text.Encoding]::GetEncoding(28591)

$abis = if ($Emulator) { [ordered]@{ 'x86_64-linux-android' = 'x86_64' } }
    else { [ordered]@{ 'aarch64-linux-android' = 'arm64-v8a'; 'armv7-linux-androideabi' = 'armeabi-v7a' } }
foreach ($target in $abis.Keys) {
    & (Join-Path $PSScriptRoot 'android-env.ps1') cargo build --release --package clock-in --manifest-path src-tauri\Cargo.toml --target $target --features tauri/custom-protocol --lib
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $target." }
    $lib = Join-Path $root "target\$target\release\libclockin_lib.so"
    # The project URL and the «Δοκιμαστικό» badge come from the same CLOCKIN_ENV,
    # so the right URL (and not the other one) means the right badge too.
    $text = $latin1.GetString([IO.File]::ReadAllBytes($lib))
    if (-not $text.Contains($wantUrl)) { throw "$target library does not talk to the $Env project." }
    if ($otherUrl -and $otherUrl -ne $wantUrl -and $text.Contains($otherUrl)) { throw "$target library contains the other project's URL." }
    $jni = Join-Path $android "app\src\main\jniLibs\$($abis[$target])"
    New-Item -ItemType Directory -Force $jni | Out-Null
    Copy-Item $lib (Join-Path $jni 'libclockin_lib.so') -Force
}
Write-Host "Checked: the libraries talk to the $Env project only$(if ($Env -eq 'prod') { ' (no test badge)' })."
# No library for other ABIs may be left over from an earlier build.
Get-ChildItem (Join-Path $android 'app\src\main\jniLibs') -Directory |
    Where-Object { $abis.Values -notcontains $_.Name } |
    ForEach-Object { Remove-Item -LiteralPath $_.FullName -Recurse -Force }

Set-Location $android
$ErrorActionPreference = 'Continue'
if ($Emulator) {
    .\gradlew.bat assembleUniversalRelease '-PabiList=x86_64' '-ParchList=x86_64' '-PtargetList=x86_64' `
        -x rustBuildUniversalRelease -x rustBuildX86_64Release --console=plain
} else {
    .\gradlew.bat assembleUniversalRelease '-PabiList=arm64-v8a,armeabi-v7a' '-ParchList=arm64,arm' '-PtargetList=aarch64,armv7' `
        -x rustBuildUniversalRelease -x rustBuildArm64Release -x rustBuildArmRelease --console=plain
}
if ($LASTEXITCODE -ne 0) { throw 'Gradle build failed.' }
$ErrorActionPreference = 'Stop'
Set-Location $root

$built = Join-Path $android 'app\build\outputs\apk\universal\release\app-universal-release.apk'
if (-not (Test-Path $built)) { throw "No signed APK at $built (was the keystore found?)." }
$name = if ($Emulator) { "clock-in-$version-dev-emulator.apk" }
    elseif ($Env -eq 'dev') { "clock-in-$version-dev.apk" } else { "clock-in-$version.apk" }
# Prod builds go next to the Windows installers, outside the repo.
$outDir = if ($Env -eq 'dev') { Join-Path $root 'target\apk' } else { "C:\dev\clock-in-releases\$version-prod" }
New-Item -ItemType Directory -Force $outDir | Out-Null
$out = Join-Path $outDir $name
Copy-Item $built $out -Force

# Check the signature (prints the certificate's SHA-256, never a password).
$buildTools = Get-ChildItem (Join-Path $env:ANDROID_HOME 'build-tools') -Directory | Sort-Object { [version]$_.Name } | Select-Object -Last 1
$ErrorActionPreference = 'Continue'
& (Join-Path $buildTools.FullName 'apksigner.bat') verify --print-certs $out | Select-String 'SHA-256'
Write-Host "APK ($Env): $out"

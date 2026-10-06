<#
.SYNOPSIS
  Development loop: builds the arm64 debug app (dev project) and installs it
  on the phone connected by USB.

.DESCRIPTION
  Builds the Rust library with cargo and packages it with Gradle directly,
  instead of `pnpm tauri android build`, which needs Windows Developer Mode
  for a symbolic link. The Tauri-generated Gradle files must already exist
  (from one `pnpm tauri android build` or `android init`).
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
$android = Join-Path $root 'src-tauri\gen\android'
if (-not (Test-Path (Join-Path $android 'app\tauri.build.gradle.kts'))) {
    throw 'Run "pnpm tauri android build" once first (it generates the Gradle glue files).'
}

$env:CLOCKIN_ENV = 'dev'
pnpm build
if ($LASTEXITCODE -ne 0) { throw 'Frontend build failed.' }
& (Join-Path $PSScriptRoot 'android-env.ps1') cargo build --package clock-in --manifest-path src-tauri\Cargo.toml --target aarch64-linux-android --features tauri/custom-protocol --lib
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }
$jni = Join-Path $android 'app\src\main\jniLibs\arm64-v8a'
New-Item -ItemType Directory -Force $jni | Out-Null
Copy-Item (Join-Path $root 'target\aarch64-linux-android\debug\libclockin_lib.so') (Join-Path $jni 'libclockin_lib.so') -Force

Set-Location $android
$ErrorActionPreference = 'Continue'
.\gradlew.bat assembleArm64Debug -x rustBuildArm64Debug --console=plain
if ($LASTEXITCODE -ne 0) { throw 'Gradle build failed.' }
& (Join-Path $env:ANDROID_HOME 'platform-tools\adb.exe') install -r 'app\build\outputs\apk\arm64\debug\app-arm64-debug.apk'

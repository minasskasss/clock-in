<#
.SYNOPSIS
  Runs a command with the environment Clock In's Android build needs on this
  Windows laptop (docs/DECISIONS.md, Phase 5).

.DESCRIPTION
  - JAVA_HOME, ANDROID_HOME and NDK_HOME: taken from the environment, or the
    Android Studio defaults if they are not set.
  - SQLCipher's vendored OpenSSL is configured with Perl and built with make.
    For an Android (Unix) target that needs MSYS2's Perl and GNU make
    (C:\msys64, `pacman -S make`); Strawberry Perl refuses. Their folder goes
    first on PATH for this command only.
  - The C compiler for each Android target is the NDK's clang.exe with an
    explicit --target, so both cargo (native) and MSYS make can run it.

.EXAMPLE
  .\tools\android-env.ps1 pnpm tauri android dev
  .\tools\android-env.ps1 cargo clippy -p clock-in --target aarch64-linux-android '--' -D warnings
  (PowerShell drops a bare -- before it reaches a script, so quote it.)
#>
$ErrorActionPreference = 'Stop'

# The minimum Android API level (tauri.conf.json bundle.android.minSdkVersion).
$MinSdk = 26

if (-not $env:JAVA_HOME) { $env:JAVA_HOME = 'C:\Program Files\Android\Android Studio\jbr' }
if (-not $env:ANDROID_HOME) { $env:ANDROID_HOME = Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
if (-not $env:NDK_HOME) {
    $ndk = Get-ChildItem (Join-Path $env:ANDROID_HOME 'ndk') -Directory -ErrorAction SilentlyContinue |
        Sort-Object { [version]$_.Name } | Select-Object -Last 1
    if (-not $ndk) { throw "No NDK found under $env:ANDROID_HOME\ndk. Install 'NDK (Side by side)' in Android Studio's SDK Manager." }
    $env:NDK_HOME = $ndk.FullName
}
foreach ($path in @("$env:JAVA_HOME\bin\java.exe", $env:ANDROID_HOME, $env:NDK_HOME)) {
    if (-not (Test-Path $path)) { throw "Not found: $path" }
}

$msys = 'C:\msys64\usr\bin'
if (-not (Test-Path "$msys\perl.exe") -or -not (Test-Path "$msys\make.exe")) {
    throw "MSYS2 Perl and make are needed (C:\msys64\usr\bin). In an MSYS2 terminal: pacman -S --needed perl make"
}
$env:PATH = "$msys;$env:PATH"

$bin = (Join-Path $env:NDK_HOME 'toolchains\llvm\prebuilt\windows-x86_64\bin') -replace '\\', '/'
$targets = @{
    'aarch64_linux_android'     = 'aarch64-linux-android'
    'armv7_linux_androideabi'   = 'armv7a-linux-androideabi'
    'i686_linux_android'        = 'i686-linux-android'
    'x86_64_linux_android'      = 'x86_64-linux-android'
}
foreach ($rust in $targets.Keys) {
    Set-Item "env:CC_$rust" "$bin/clang.exe"
    Set-Item "env:CXX_$rust" "$bin/clang++.exe"
    Set-Item "env:CFLAGS_$rust" "--target=$($targets[$rust])$MinSdk"
    Set-Item "env:CXXFLAGS_$rust" "--target=$($targets[$rust])$MinSdk"
    Set-Item "env:AR_$rust" "$bin/llvm-ar.exe"
    Set-Item "env:RANLIB_$rust" "$bin/llvm-ranlib.exe"
    Set-Item "env:CARGO_TARGET_$($rust.ToUpper())_LINKER" "$bin/$($targets[$rust])$MinSdk-clang.cmd"
}

if ($args.Count -eq 0) {
    Write-Host "Android build environment ready (JAVA_HOME, ANDROID_HOME, NDK_HOME = $env:NDK_HOME)."
    exit 0
}
$command = $args[0]
$rest = @($args | Select-Object -Skip 1)
# Build tools write progress to stderr; that must not count as a failure.
$ErrorActionPreference = 'Continue'
& $command @rest
exit $LASTEXITCODE

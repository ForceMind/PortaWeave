# Real MSVC-compiled C executable linked to the EXISTING portaweave-native
# Rust cdylib, on a disposable Windows CI runner (not a WinFsp mount).
$ErrorActionPreference = 'Stop'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vcvars = ''
if (Test-Path $vswhere) {
    $install = (& $vswhere -latest -products '*' -property installationPath | Select-Object -First 1)
    if ($install) { $vcvars = Join-Path $install 'VC\Auxiliary\Build\vcvars64.bat' }
}
if (-not $vcvars -or -not (Test-Path $vcvars)) {
    foreach ($base in @(${env:ProgramFiles}, ${env:ProgramFiles(x86)})) {
        $candidate = Join-Path $base 'Microsoft Visual Studio'
        if (-not (Test-Path $candidate)) { continue }
        $found = Get-ChildItem -Path $candidate -Filter vcvars64.bat -File -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $vcvars = $found.FullName; break }
    }
}
if (-not $vcvars -or -not (Test-Path $vcvars)) { throw 'Cannot locate a supported MSVC toolchain' }
Write-Host "MSVC environment: $vcvars"
$root = (Resolve-Path '.').Path
$include = Join-Path $root 'include'
$source = Join-Path $root 'tools\native_abi_smoke.c'
$libdir = Join-Path $root 'target\debug'
$library = Get-ChildItem -Path $libdir -Filter 'portaweave_native*.lib' -File | Select-Object -First 1
if (-not $library) { throw 'portaweave-native import library is missing' }
$exe = Join-Path $env:RUNNER_TEMP 'pw-native-abi-msvc.exe'
$newDir = Join-Path $env:RUNNER_TEMP "pw-native-abi-msvc-$env:GITHUB_RUN_ID"
$bat = Join-Path $env:RUNNER_TEMP 'run-native-abi-msvc.bat'
$commands = @(
    '@echo off'
    "call `"$vcvars`""
    'if errorlevel 1 exit /b 1'
    "cl /nologo /W4 /WX /utf-8 /I`"$include`" `"$source`" /link /LIBPATH:`"$libdir`" `"$($library.Name)`" /OUT:`"$exe`""
    'if errorlevel 1 exit /b 1'
    "set `"PATH=$libdir;%PATH%`""
    "`"$exe`" `"$newDir`""
    'if errorlevel 1 exit /b 1'
)
Set-Content -Path $bat -Value $commands -Encoding ascii
& cmd.exe /d /c $bat
if ($LASTEXITCODE -ne 0) { throw "MSVC C ABI smoke failed: $LASTEXITCODE" }

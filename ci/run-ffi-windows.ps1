# Run the exact same C caller against the Rust DLL on a disposable
# GitHub Actions Windows runner. This does not install WinFsp or create a mount.
$ErrorActionPreference = 'Stop'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $vswhere)) { throw 'Visual Studio vswhere is unavailable' }
$install = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1)
if ($LASTEXITCODE -ne 0 -or -not $install) { throw 'MSVC native toolchain not found' }
$vcvars = Join-Path $install 'VC\Auxiliary\Build\vcvars64.bat'
if (-not (Test-Path $vcvars)) { throw 'MSVC vcvars64.bat not found' }
$root = (Resolve-Path '.').Path
$include = Join-Path $root 'crates\ffi\include'
$source = Join-Path $root 'crates\ffi\tests\abi_smoke.c'
$libdir = Join-Path $root 'target\release'
$library = Get-ChildItem -Path $libdir -Filter 'portaweave_ffi*.lib' -File | Select-Object -First 1
if (-not $library) { throw 'Rust cdylib import library not found' }
$exe = Join-Path $env:RUNNER_TEMP 'portaweave-ffi-c.exe'
$newContainer = Join-Path $env:RUNNER_TEMP "portaweave-ffi-windows-$env:GITHUB_RUN_ID"
$batch = Join-Path $env:RUNNER_TEMP 'run-ffi-native.bat'
$commands = @(
    '@echo off'
    "call `"$vcvars`""
    'if errorlevel 1 exit /b 1'
    "cl /nologo /W4 /WX /utf-8 /I`"$include`" `"$source`" /link /LIBPATH:`"$libdir`" `"$($library.Name)`" /OUT:`"$exe`""
    'if errorlevel 1 exit /b 1'
    "set `"PATH=$libdir;%PATH%`""
    "`"$exe`" `"$newContainer`""
    'if errorlevel 1 exit /b 1'
)
Set-Content -Path $batch -Value $commands -Encoding ascii
& cmd.exe /d /c $batch
if ($LASTEXITCODE -ne 0) { throw "C ABI harness failed with exit code $LASTEXITCODE" }

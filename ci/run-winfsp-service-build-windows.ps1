# Compile service against pinned v2.1 WinFsp headers and run --self-test only.
# NO WinFsp driver install, NO call to --lab-mount in CI.
$ErrorActionPreference='Stop'
$vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $vswhere)) { throw 'MSVC discovery unavailable' }
$install=(& $vswhere -latest -products '*' -property installationPath | Select-Object -First 1)
$vcvars=Join-Path $install 'VC\Auxiliary\Build\vcvars64.bat'
if (-not (Test-Path $vcvars)) {throw 'MSVC x64 toolchain unavailable'}
$root=(Resolve-Path '.').Path
$inc=Join-Path $root 'include'
$sdk=Join-Path $root 'build-deps\winfsp\inc'
$source=Join-Path $root 'platform\windows\winfsp_service.c'
$libdir=Join-Path $root 'target\debug'
$lib=Get-ChildItem -Path $libdir -Filter 'portaweave_native*.lib' -File |
  Sort-Object Name | Select-Object -First 1
if (-not $lib) {throw 'Native Rust DLL import lib unavailable'}
$exe=Join-Path $env:RUNNER_TEMP 'pw-winfsp-service.exe'
$bat=Join-Path $env:RUNNER_TEMP 'pw-build-winfsp-service.bat'
$cmds=@(
 '@echo off'
 "call `"$vcvars`""
 'if errorlevel 1 exit /b 1'
 "cl /nologo /W3 /WX /utf-8 /I`"$inc`" /I`"$sdk`" `"$source`" /link /LIBPATH:`"$libdir`" `"$($lib.Name)`" /OUT:`"$exe`""
 'if errorlevel 1 exit /b 1'
 "set `"PATH=$libdir;%PATH%`""
 "`"$exe`" --self-test"
 'if errorlevel 1 exit /b 1'
)
Set-Content -Path $bat -Value $cmds -Encoding ascii
& cmd.exe /d /c $bat
if ($LASTEXITCODE -ne 0) {throw "WinFsp service selftest failed $LASTEXITCODE"}

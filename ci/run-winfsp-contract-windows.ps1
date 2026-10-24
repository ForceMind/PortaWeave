# Compile/run M0-G WinFsp 2.1 callback ABI CONTRACT with a mock WinFsp
# FSP_FILE_SYSTEM object. No driver installation, no mounted volume.
$ErrorActionPreference = 'Stop'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vcvars = ''
if (Test-Path $vswhere) {
    $install = (& $vswhere -latest -products '*' -property installationPath | Select-Object -First 1)
    if ($install) { $vcvars = Join-Path $install 'VC\Auxiliary\Build\vcvars64.bat' }
}
if (-not $vcvars -or -not (Test-Path $vcvars)) {
    throw "MSVC vcvars64.bat unavailable"
}
$root = (Resolve-Path '.').Path
$sdk = Join-Path $root 'build-deps\winfsp\inc'
if (-not (Test-Path (Join-Path $sdk 'winfsp\winfsp.h'))) {
    throw 'Pinned WinFsp SDK headers missing'
}
$include = Join-Path $root 'include'
$source = Join-Path $root 'platform\windows\winfsp_contract.c'
$libdir = Join-Path $root 'target\debug'
$lib = Join-Path $libdir 'portaweave_native.lib'
if (-not (Test-Path $lib)) { throw 'Rust native DLL import library missing' }
$exe = Join-Path $env:RUNNER_TEMP 'portaweave-winfsp-contract.exe'
$newDir = Join-Path $env:RUNNER_TEMP "pw-winfsp-contract-$env:GITHUB_RUN_ID"
$batch = Join-Path $env:RUNNER_TEMP 'run-winfsp-contract.bat'
$commands = @(
    '@echo off'
    "call `"$vcvars`""
    'if errorlevel 1 exit /b 1'
    "cl /nologo /W3 /WX /utf-8 /I`"$include`" /I`"$sdk`" `"$source`" /link `"$lib`" /OUT:`"$exe`""
    'if errorlevel 1 exit /b 1'
    "set `"PATH=$libdir;%PATH%`""
    "`"$exe`" `"$newDir`""
    'if errorlevel 1 exit /b 1'
)
Set-Content -Path $batch -Value $commands -Encoding ascii
& cmd.exe /d /c $batch
if ($LASTEXITCODE -ne 0) { throw "WinFsp callback contract failed: $LASTEXITCODE" }

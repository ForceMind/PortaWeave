# Run the exact same C caller against the Rust DLL on a disposable
# GitHub Actions Windows runner. This does not install WinFsp or create a mount.
$ErrorActionPreference = 'Stop'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $vswhere)) { throw 'Visual Studio vswhere is unavailable' }
# The installed MSVC toolchain is known to work for rustc, but its
# vswhere workload component tags vary across runner images. Discover the
# actual vcvars script rather than assuming a specific workload identifier.
$install = (& $vswhere -latest -products '*' -property installationPath | Select-Object -First 1)
$vcvars = if ($install) { Join-Path $install 'VC\Auxiliary\Build\vcvars64.bat' } else { '' }
if (-not $vcvars -or -not (Test-Path $vcvars)) {
    $candidates = @(
        (Join-Path ${env:ProgramFiles} 'Microsoft Visual Studio'),
        (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio')
    )
    foreach ($candidate in $candidates) {
        if (-not (Test-Path $candidate)) { continue }
        $found = Get-ChildItem -Path $candidate -Filter 'vcvars64.bat' -File -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $vcvars = $found.FullName; break }
    }
}
if (-not $vcvars -or -not (Test-Path $vcvars)) { throw 'MSVC vcvars64.bat not found (no matching VS toolchain)' }
Write-Host "Using vcvars: $vcvars"
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

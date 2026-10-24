# Read-only inventory on a disposable CI runner. No install, DLL load,
# service start/configuration, drive-letter assignment or mount is attempted.
$ErrorActionPreference = 'Stop'
$runtimes = @()
foreach ($base in @(${env:ProgramFiles(x86)}, $env:ProgramFiles)) {
    if (-not $base) { continue }
    foreach ($name in @('winfsp-x64.dll', 'winfsp_x64.dll')) {
        $path = Join-Path $base "WinFsp\bin\$name"
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $file = Get-Item -LiteralPath $path
            $runtimes += [ordered]@{ path = $file.FullName; version = $file.VersionInfo.FileVersion }
        }
    }
}
$drivers = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name LIKE 'WinFsp%'" |
    Select-Object Name, State, StartMode, PathName)
$prerequisites = $runtimes.Count -gt 0 -and $drivers.Count -gt 0
$reason = if ($prerequisites) {
    'Runtime and driver registered; actual mount/permissions/sync are still NOT_RUN.'
} else {
    'BLOCKED: installed runtime and/or registered WinFsp driver absent. No installation attempted.'
}
$report = [ordered]@{
    scope = 'read-only CI Windows mount environment inventory'
    source_commit = $env:GITHUB_SHA
    os_version = [Environment]::OSVersion.Version.ToString()
    architecture = $env:PROCESSOR_ARCHITECTURE
    runtimes = $runtimes
    drivers = $drivers
    mount_prerequisites_present = $prerequisites
    actual_mount = 'NOT_RUN'
    reason = $reason
    installation_attempted = $false
    service_change_attempted = $false
    mount_attempted = $false
    development_volume_certified = $false
}
$target = Join-Path $PSScriptRoot '..\target'
New-Item -ItemType Directory -Path $target -Force | Out-Null
$path = Join-Path $target 'windows-mount-environment.json'
$report | ConvertTo-Json -Depth 6 | Out-File -LiteralPath $path -NoClobber -Encoding utf8
Write-Host $reason

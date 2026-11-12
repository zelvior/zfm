#Requires -Version 5.1
param([Parameter(Mandatory=$true)][string]$Installer)
$ErrorActionPreference = 'Stop'
$install = Join-Path $env:RUNNER_TEMP ('zfm-install-smoke-' + [Guid]::NewGuid().ToString('N'))
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'ZFM\ZFM File Manager.lnk'
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\ZFM'
$uninstaller = Join-Path $install 'Uninstall.exe'
$conflict = Join-Path $env:RUNNER_TEMP ('zfm-install-conflict-' + [Guid]::NewGuid().ToString('N'))
try {
    New-Item -ItemType Directory -Path $conflict | Out-Null
    $existing = Join-Path $conflict 'zfm.exe'
    Set-Content -LiteralPath $existing -Value 'unrelated file'
    $blocked = Start-Process -FilePath (Resolve-Path -LiteralPath $Installer).Path -ArgumentList @('/S', "/D=$conflict") -Wait -PassThru
    if ($blocked.ExitCode -eq 0) { throw 'Installer accepted a folder containing an unrelated zfm.exe.' }
    if ((Get-Content -LiteralPath $existing -Raw).Trim() -ne 'unrelated file') { throw 'Installer overwrote an unrelated file.' }
    $run = Start-Process -FilePath (Resolve-Path -LiteralPath $Installer).Path -ArgumentList @('/S', "/D=$install") -Wait -PassThru
    if ($run.ExitCode -ne 0) { throw "Installer failed: $($run.ExitCode)" }
    foreach ($path in @((Join-Path $install 'zfm.exe'), $uninstaller, $shortcut, $key)) {
        if (-not (Test-Path -LiteralPath $path)) { throw "Installer did not create $path" }
    }
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
    if ($link.TargetPath -ne (Join-Path $install 'zfm.exe')) { throw 'Start menu shortcut points to the wrong executable.' }
    & ./tests/gui-smoke-windows.ps1 -Executable (Join-Path $install 'zfm.exe')
    $sentinel = Join-Path $install 'user-data.txt'
    Set-Content -LiteralPath $sentinel -Value 'must survive uninstall'
    $run = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
    if ($run.ExitCode -ne 0) { throw "Uninstaller failed: $($run.ExitCode)" }
    if (Test-Path -LiteralPath (Join-Path $install 'zfm.exe')) { throw 'Uninstaller left the executable behind.' }
    if (Test-Path -LiteralPath $shortcut) { throw 'Uninstaller left the shortcut behind.' }
    if (Test-Path -LiteralPath $key) { throw 'Uninstaller left the registry entry behind.' }
    if (-not (Test-Path -LiteralPath $sentinel)) { throw 'Uninstaller removed unrelated user data.' }
    Write-Host 'Installer, installed application, shortcut and uninstaller smoke tests passed.'
} finally {
    if (Test-Path -LiteralPath $uninstaller) {
        Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -ErrorAction SilentlyContinue | Out-Null
    }
    Remove-Item -LiteralPath $install -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $conflict -Recurse -Force -ErrorAction SilentlyContinue
}

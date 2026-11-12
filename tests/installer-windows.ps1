#Requires -Version 5.1
# Smoke test for the ZFM setup program: silent install, registry entries,
# Start Menu shortcuts, reinstalling, and the uninstaller's promise to keep the
# user's own files. Runs against the packaged download, so it also proves that
# the shipped file is a working installer.
param([Parameter(Mandatory = $true)][string]$Installer)
$ErrorActionPreference = 'Stop'

$setup = (Resolve-Path -LiteralPath $Installer).Path
$root = Join-Path $env:RUNNER_TEMP ('zfm-setup-smoke-' + [Guid]::NewGuid().ToString('N'))
$first = Join-Path $root 'first'
$second = Join-Path $root 'second'
$conflict = Join-Path $root 'conflict'
$managed = @('zfm.exe', 'zfm.ico', 'LICENSE.txt', 'README.txt', 'Uninstall.exe')
$menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'ZFM'
$appShortcut = Join-Path $menu 'ZFM File Manager.lnk'
$uninstallShortcut = Join-Path $menu 'Uninstall ZFM.lnk'
$appKey = 'HKCU:\Software\ZFM'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\ZFM'

function Invoke-Setup {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [string[]]$Arguments = @(),
        [int]$Timeout = 120
    )
    $process = Start-Process -FilePath $Executable -ArgumentList $Arguments -PassThru
    if (-not $process.WaitForExit($Timeout * 1000)) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        throw "$Executable did not finish within $Timeout seconds."
    }
    return $process.ExitCode
}

function Assert-ExitCode {
    param([int]$Actual, [int]$Expected, [string]$What)
    if ($Actual -ne $Expected) {
        throw "$What exited with $Actual, expected $Expected."
    }
}

function Wait-Until {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Condition,
        [string]$What = 'the expected state',
        [int]$Timeout = 60
    )
    $deadline = (Get-Date).AddSeconds($Timeout)
    while ((Get-Date) -lt $deadline) {
        if (& $Condition) { return }
        Start-Sleep -Milliseconds 200
    }
    throw "Timed out after $Timeout seconds waiting for $What."
}

function Assert-Installed {
    param([string]$Folder)
    foreach ($name in $managed) {
        $path = Join-Path $Folder $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Setup did not create $path"
        }
    }
    foreach ($path in @($appShortcut, $uninstallShortcut, $appKey, $uninstallKey)) {
        if (-not (Test-Path -LiteralPath $path)) {
            throw "Setup did not create $path"
        }
    }
    if ((Get-ItemProperty -LiteralPath $appKey).InstallDir -ne $Folder) {
        throw "Setup recorded the wrong installation folder for $Folder"
    }
}

function Assert-Uninstalled {
    param([string]$Folder)
    foreach ($name in $managed) {
        $path = Join-Path $Folder $name
        if (Test-Path -LiteralPath $path) {
            throw "The uninstaller left $path behind."
        }
    }
    foreach ($path in @($appShortcut, $uninstallShortcut, $appKey, $uninstallKey)) {
        if (Test-Path -LiteralPath $path) {
            throw "The uninstaller left $path behind."
        }
    }
}

try {
    New-Item -ItemType Directory -Path $root -Force | Out-Null

    # 1. Setup refuses to replace files it does not own and writes nothing.
    New-Item -ItemType Directory -Path $conflict | Out-Null
    $intruder = Join-Path $conflict 'zfm.exe'
    Set-Content -LiteralPath $intruder -Value 'unrelated file'
    $code = Invoke-Setup $setup @('/S', "/D=`"$conflict`"")
    Assert-ExitCode $code 1 'Setup in an occupied folder'
    if ((Get-Content -LiteralPath $intruder -Raw).Trim() -ne 'unrelated file') {
        throw 'Setup overwrote an unrelated zfm.exe.'
    }
    if (Test-Path -LiteralPath $appKey) {
        throw 'A refused installation still wrote to the registry.'
    }

    # 2. Bad usage exits with code 2 and, because of /S, without a dialog.
    $code = Invoke-Setup $setup @('/S', '/NOPE')
    Assert-ExitCode $code 2 'Setup with an unknown option'

    # 3. Silent install into the requested folder.
    $code = Invoke-Setup $setup @('/S', "/D=`"$first`"")
    Assert-ExitCode $code 0 'Silent setup'
    Assert-Installed $first
    $app = Get-ItemProperty -LiteralPath $appKey
    if ([string]::IsNullOrWhiteSpace($app.Version)) {
        throw 'Setup did not record a version.'
    }
    $entry = Get-ItemProperty -LiteralPath $uninstallKey
    $uninstaller = Join-Path $first 'Uninstall.exe'
    if ($entry.DisplayName -ne 'ZFM File Manager') {
        throw "Unexpected Programs and Features name '$($entry.DisplayName)'."
    }
    if ([string]::IsNullOrWhiteSpace($entry.DisplayVersion)) {
        throw 'The Programs and Features entry has no version.'
    }
    if ($entry.Publisher -ne 'ZFM contributors') {
        throw "Unexpected publisher '$($entry.Publisher)'."
    }
    if ($entry.InstallLocation -ne $first) {
        throw "InstallLocation is '$($entry.InstallLocation)', expected '$first'."
    }
    if ($entry.DisplayIcon -ne (Join-Path $first 'zfm.ico')) {
        throw "DisplayIcon is '$($entry.DisplayIcon)'."
    }
    if ($entry.UninstallString -ne ('"' + $uninstaller + '"')) {
        throw "UninstallString is '$($entry.UninstallString)'."
    }
    if ($entry.QuietUninstallString -ne ('"' + $uninstaller + '" /S')) {
        throw "QuietUninstallString is '$($entry.QuietUninstallString)'."
    }
    if ($entry.NoModify -ne 1 -or $entry.NoRepair -ne 1) {
        throw 'The entry must be listed as neither modifiable nor repairable.'
    }
    if ($entry.EstimatedSize -le 0) {
        throw 'The entry has no estimated installed size.'
    }

    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($appShortcut)
    if ($link.TargetPath -ne (Join-Path $first 'zfm.exe')) {
        throw "The Start Menu shortcut points at '$($link.TargetPath)'."
    }
    if ($link.WorkingDirectory -ne $first) {
        throw "The Start Menu shortcut works from '$($link.WorkingDirectory)'."
    }
    $removeLink = $shell.CreateShortcut($uninstallShortcut)
    if ($removeLink.TargetPath -ne $uninstaller) {
        throw "The uninstall shortcut points at '$($removeLink.TargetPath)'."
    }

    # 4. The installed application really starts.
    & ./tests/gui-smoke-windows.ps1 -Executable (Join-Path $first 'zfm.exe')

    # 5. Running setup again over its own folder is an upgrade, not a conflict.
    $code = Invoke-Setup $setup @('/S', "/D=`"$first`"")
    Assert-ExitCode $code 0 'Reinstalling over an existing installation'
    Assert-Installed $first

    # 6. The uninstaller removes its own files and leaves the user's files.
    $notes = Join-Path $first 'notes.txt'
    Set-Content -LiteralPath $notes -Value 'must survive uninstall'
    $code = Invoke-Setup $uninstaller @('/S')
    Assert-ExitCode $code 0 'Silent uninstall'
    # The uninstaller hands the removal to a copy of itself so that its own file
    # is not locked, which means the work finishes shortly after it exits.
    Wait-Until { try { Assert-Uninstalled $first; $true } catch { $false } } 'the uninstaller to finish'
    Assert-Uninstalled $first
    if (-not (Test-Path -LiteralPath $notes)) {
        throw "The uninstaller removed the user's own file."
    }

    # 7. A second install and uninstall removes the emptied folder as well.
    $code = Invoke-Setup $setup @('/S', "/D=`"$second`"")
    Assert-ExitCode $code 0 'Silent setup into a second folder'
    Assert-Installed $second
    $code = Invoke-Setup (Join-Path $second 'Uninstall.exe') @('/S')
    Assert-ExitCode $code 0 'Silent uninstall of the second folder'
    Wait-Until { -not (Test-Path -LiteralPath $second) } 'the empty installation folder to be removed'
    if (Test-Path -LiteralPath $second) {
        throw 'The uninstaller left an empty installation folder behind.'
    }

    # 8. /UNINSTALL lets the setup program itself remove an installation.
    $code = Invoke-Setup $setup @('/S', "/D=`"$second`"")
    Assert-ExitCode $code 0 'Silent setup before /UNINSTALL'
    $code = Invoke-Setup $setup @('/UNINSTALL', '/S')
    Assert-ExitCode $code 0 'Silent /UNINSTALL'
    Wait-Until { -not (Test-Path -LiteralPath $second) } 'the /UNINSTALL removal to finish'
    if (Test-Path -LiteralPath $second) {
        throw 'Setup /UNINSTALL left the installation folder behind.'
    }

    Write-Host 'Setup, installed application, shortcuts, registry entries and uninstaller all behave as documented.'
} finally {
    $leftover = Join-Path $second 'Uninstall.exe'
    if (Test-Path -LiteralPath $leftover) {
        try { Invoke-Setup $leftover @('/S') -Timeout 60 | Out-Null } catch { }
    }
    foreach ($key in @($uninstallKey, $appKey)) {
        Remove-Item -LiteralPath $key -Recurse -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $menu -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

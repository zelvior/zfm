#Requires -Version 5.1
# Smoke test for the ZFM setup program: interactive wizard flow, silent install,
# registry entries, Start Menu shortcuts, reinstalling, recovering an
# interrupted install, and the uninstaller's promise to keep the user's own
# files. Runs against the packaged download, so it also proves that the shipped
# file is a working installer.
param([Parameter(Mandatory = $true)][string]$Installer)
$ErrorActionPreference = 'Stop'

if (-not ('ZfmSetupSmoke' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class ZfmSetupSmoke {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int left, top, right, bottom; }
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern IntPtr GetDlgItem(IntPtr parent, int id);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern int GetClassNameW(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageTimeoutW", ExactSpelling=true)]
    public static extern IntPtr SendCommand(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam, uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageTimeoutW", ExactSpelling=true)]
    public static extern IntPtr SendSetText(IntPtr window, uint message, UIntPtr wparam, [MarshalAs(UnmanagedType.LPWStr)] string lparam, uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageTimeoutW", ExactSpelling=true)]
    public static extern IntPtr SendGetText(IntPtr window, uint message, UIntPtr wparam, [MarshalAs(UnmanagedType.LPWStr)] StringBuilder lparam, uint flags, uint timeout, out UIntPtr result);
}
'@
}

$setup = (Resolve-Path -LiteralPath $Installer).Path
$tempBase = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$root = Join-Path $tempBase ('zfm-setup-smoke-' + [Guid]::NewGuid().ToString('N'))
$first = Join-Path $root 'first'
$second = Join-Path $root 'second'
$wizard = Join-Path $root 'wizard'
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

function Send-WizardCommand {
    param([IntPtr]$Window, [int]$Id)
    $ack = [UIntPtr]::Zero
    if ([ZfmSetupSmoke]::SendCommand($Window, 0x0111, [UIntPtr]$Id, [IntPtr]::Zero, 2, 5000, [ref]$ack) -eq [IntPtr]::Zero) {
        throw "Setup window did not respond to command $Id."
    }
}

function Set-WizardText {
    param([IntPtr]$Control, [string]$Value)
    $ack = [UIntPtr]::Zero
    if ([ZfmSetupSmoke]::SendSetText($Control, 0x000C, [UIntPtr]::Zero, $Value, 2, 3000, [ref]$ack) -eq [IntPtr]::Zero) {
        throw 'Setup control did not respond to WM_SETTEXT.'
    }
}

function Get-WizardText {
    param([IntPtr]$Control)
    $buffer = New-Object System.Text.StringBuilder 1024
    $copied = [UIntPtr]::Zero
    if ([ZfmSetupSmoke]::SendGetText($Control, 0x000D, [UIntPtr]$buffer.Capacity, $buffer, 2, 3000, [ref]$copied) -eq [IntPtr]::Zero) {
        throw 'Setup control did not respond to WM_GETTEXT.'
    }
    return $buffer.ToString()
}

function Invoke-InteractiveSetup {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$TargetFolder
    )
    $wizardProcess = Start-Process -FilePath $Executable -PassThru
    try {
        $window = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $wizardProcess.Refresh()
            if ($wizardProcess.HasExited) {
                throw "Interactive setup exited before showing its window (exit code $($wizardProcess.ExitCode))."
            }
            $window = $wizardProcess.MainWindowHandle
            if ($window -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($window -eq [IntPtr]::Zero) {
            throw 'Interactive setup did not show its window within 10 seconds.'
        }
        $className = New-Object System.Text.StringBuilder 256
        [void][ZfmSetupSmoke]::GetClassNameW($window, $className, $className.Capacity)
        if ($className.ToString() -ne 'ZFMSetupWindow') {
            throw "Expected ZFMSetupWindow, got '$className'."
        }

        $back = [ZfmSetupSmoke]::GetDlgItem($window, 101)
        $next = [ZfmSetupSmoke]::GetDlgItem($window, 102)
        $browse = [ZfmSetupSmoke]::GetDlgItem($window, 104)
        $pathEdit = [ZfmSetupSmoke]::GetDlgItem($window, 105)
        $licenseEdit = [ZfmSetupSmoke]::GetDlgItem($window, 106)
        $launch = [ZfmSetupSmoke]::GetDlgItem($window, 107)
        $status = [ZfmSetupSmoke]::GetDlgItem($window, 108)
        $body = [ZfmSetupSmoke]::GetDlgItem($window, 110)
        foreach ($handle in @($back, $next, $browse, $pathEdit, $licenseEdit, $launch, $status, $body)) {
            if ($handle -eq [IntPtr]::Zero) {
                throw 'Setup window is missing an expected wizard control.'
            }
        }

        $bodyRect = New-Object 'ZfmSetupSmoke+Rect'
        if (-not [ZfmSetupSmoke]::GetWindowRect($body, [ref]$bodyRect) -or ($bodyRect.bottom - $bodyRect.top) -lt 64) {
            throw 'Wizard body control is too short for a three-line description.'
        }

        # Welcome -> Folder
        if ((Get-WizardText $next) -ne 'Next' -or [ZfmSetupSmoke]::IsWindowVisible($pathEdit)) {
            throw 'Unexpected initial wizard page state.'
        }
        Send-WizardCommand $window 102
        if (-not [ZfmSetupSmoke]::IsWindowVisible($pathEdit) -or -not [ZfmSetupSmoke]::IsWindowVisible($browse)) {
            throw 'Wizard did not advance to the folder page.'
        }
        $statusRect = New-Object 'ZfmSetupSmoke+Rect'
        if (-not [ZfmSetupSmoke]::GetWindowRect($status, [ref]$statusRect) -or ($statusRect.bottom - $statusRect.top) -lt 90) {
            throw 'Folder page status area is too short and would clip multiline messages.'
        }

        # Folder validation preserves typed text and reports errors inline.
        Set-WizardText $pathEdit 'relative\folder'
        Send-WizardCommand $window 102
        if (-not [ZfmSetupSmoke]::IsWindowVisible($pathEdit)) {
            throw 'Wizard accepted a relative installation folder.'
        }
        if ((Get-WizardText $pathEdit) -ne 'relative\folder') {
            throw 'Wizard overwrote the typed folder on validation error.'
        }
        if ([string]::IsNullOrWhiteSpace((Get-WizardText $status))) {
            throw 'Wizard did not explain why the folder was rejected.'
        }

        # Folder -> License
        Set-WizardText $pathEdit $TargetFolder
        Send-WizardCommand $window 102
        if (-not [ZfmSetupSmoke]::IsWindowVisible($licenseEdit) -or (Get-WizardText $next) -ne 'Install') {
            throw "Wizard did not advance to the license page (status: $(Get-WizardText $status))."
        }

        # License -> Installing -> Finish (runs on the background worker thread)
        Send-WizardCommand $window 102
        $finished = $false
        $deadline = (Get-Date).AddSeconds(60)
        while ((Get-Date) -lt $deadline) {
            $wizardProcess.Refresh()
            if ($wizardProcess.HasExited) {
                throw "Interactive setup exited unexpectedly during installation (exit code $($wizardProcess.ExitCode))."
            }
            if (-not [ZfmSetupSmoke]::IsWindowEnabled($window)) {
                $reason = Get-WizardText $status
                throw "Interactive setup opened a modal error dialog during installation: $reason"
            }
            if ([ZfmSetupSmoke]::IsWindowVisible($launch) -and (Get-WizardText $next) -eq 'Finish') {
                $finished = $true
                break
            }
            Start-Sleep -Milliseconds 100
        }
        if (-not $finished) {
            throw "Interactive setup did not reach the Finish page within 60 seconds (status: $(Get-WizardText $status))."
        }

        # Finish -> close without launching ZFM
        Send-WizardCommand $window 102
        if (-not $wizardProcess.WaitForExit(30000)) {
            throw 'Interactive setup did not exit after clicking Finish.'
        }
        Assert-ExitCode $wizardProcess.ExitCode 0 'Interactive wizard setup'
    } finally {
        $wizardProcess.Refresh()
        if (-not $wizardProcess.HasExited) {
            Stop-Process -Id $wizardProcess.Id -Force -ErrorAction SilentlyContinue
            $wizardProcess.WaitForExit()
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

    # 9. Interactive wizard flow exercises the background worker thread and its
    # COM shortcut creation end-to-end without /S.
    Invoke-InteractiveSetup -Executable $setup -TargetFolder $wizard
    Assert-Installed $wizard
    $wizardLink = $shell.CreateShortcut($appShortcut)
    if ($wizardLink.TargetPath -ne (Join-Path $wizard 'zfm.exe')) {
        throw "Interactive wizard shortcut points at '$($wizardLink.TargetPath)'."
    }

    # 10. An incomplete prior ZFM installation (payload copied, registry and
    # shortcuts absent) is recognized and completed cleanly on rerun.
    foreach ($key in @($uninstallKey, $appKey)) {
        Remove-Item -LiteralPath $key -Recurse -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $menu -Recurse -Force -ErrorAction SilentlyContinue
    $code = Invoke-Setup $setup @('/S', "/D=`"$wizard`"")
    Assert-ExitCode $code 0 'Completing an unregistered prior ZFM installation'
    Assert-Installed $wizard
    $code = Invoke-Setup (Join-Path $wizard 'Uninstall.exe') @('/S')
    Assert-ExitCode $code 0 'Silent uninstall after interactive setup'
    Wait-Until { -not (Test-Path -LiteralPath $wizard) } 'the interactive installation folder to be removed'

    Write-Host 'Interactive wizard, silent setup, installed application, shortcuts, registry entries and uninstaller all behave as documented.'
} finally {
    foreach ($folder in @($second, $wizard)) {
        $leftover = Join-Path $folder 'Uninstall.exe'
        if (Test-Path -LiteralPath $leftover) {
            try { Invoke-Setup $leftover @('/S') -Timeout 60 | Out-Null } catch { }
        }
    }
    foreach ($key in @($uninstallKey, $appKey)) {
        Remove-Item -LiteralPath $key -Recurse -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $menu -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

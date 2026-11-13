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
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    private static extern IntPtr LoadLibraryExW(string path, IntPtr file, uint flags);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    private static extern IntPtr FindResourceW(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern uint SizeofResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern IntPtr LockResource(IntPtr resource);
    [DllImport("kernel32.dll")]
    private static extern bool FreeLibrary(IntPtr module);
    public static byte[] Manifest(string path) {
        IntPtr module = LoadLibraryExW(path, IntPtr.Zero, 2); // LOAD_LIBRARY_AS_DATAFILE
        if (module == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        try {
            IntPtr resource = FindResourceW(module, new IntPtr(1), new IntPtr(24)); // RT_MANIFEST
            if (resource == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            int size = checked((int)SizeofResource(module, resource));
            IntPtr data = LockResource(LoadResource(module, resource));
            if (size == 0 || data == IntPtr.Zero) throw new Exception("Empty manifest resource");
            byte[] bytes = new byte[size]; Marshal.Copy(data, bytes, 0, size); return bytes;
        } finally { FreeLibrary(module); }
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int left, top, right, bottom; }
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern IntPtr GetDlgItem(IntPtr parent, int id);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern int GetClassNameW(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern IntPtr FindWindowW(string className, string title);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern bool PostMessageW(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam);
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

function Assert-SetupMetadata {
    param([string]$Path)
    $info = [Diagnostics.FileVersionInfo]::GetVersionInfo($Path)
    if ($info.CompanyName -ne 'Zelvior Labs' -or $info.FileDescription -ne 'ZFM Setup' -or $info.ProductName -ne 'ZFM - Zelvior File Manager') {
        throw "Missing setup branding in ${Path}: $($info.CompanyName), $($info.FileDescription), $($info.ProductName)"
    }
    if ($env:ZFM_VERSION -and $info.ProductVersion -ne $env:ZFM_VERSION) {
        throw "Incorrect ProductVersion '$($info.ProductVersion)' in $Path"
    }
    $bytes = [ZfmSetupSmoke]::Manifest($Path)
    $reader = [IO.StreamReader]::new([IO.MemoryStream]::new($bytes), [Text.Encoding]::UTF8, $true)
    try { [xml]$xml = $reader.ReadToEnd() } finally { $reader.Dispose() }
    $level = $xml.SelectSingleNode("//*[local-name()='requestedExecutionLevel']")
    if ($null -eq $level -or $level.GetAttribute('level') -ne 'asInvoker' -or $level.GetAttribute('uiAccess') -ne 'false') {
        throw "Setup manifest in $Path does not request asInvoker."
    }
    $ids = @($xml.SelectNodes("//*[local-name()='supportedOS']") | ForEach-Object { $_.GetAttribute('Id').ToLowerInvariant() })
    foreach ($id in @('{35138b9a-5d96-4fbd-8e2d-a2440225f93a}', '{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}')) {
        if ($id -notin $ids) { throw "Setup manifest in $Path is missing supported OS $id." }
    }
}

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
    $readme = Get-Content -Raw -LiteralPath (Join-Path $Folder 'README.txt')
    $license = Get-Content -Raw -LiteralPath (Join-Path $Folder 'LICENSE.txt')
    if ($license -notmatch 'Apache License' -or $readme -notmatch 'Upstream Rust 1.77.2 COPYRIGHT' -or $readme -notmatch 'The Rust Project is dual-licensed') {
        throw 'Installed license or upstream Rust notice is missing.'
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
        [Parameter(Mandatory = $true)][string]$TargetFolder,
        [switch]$LockThenRetry
    )
    $lockedFile = $null
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
        $heading = [ZfmSetupSmoke]::GetDlgItem($window, 109)
        $body = [ZfmSetupSmoke]::GetDlgItem($window, 110)
        foreach ($handle in @($back, $next, $browse, $pathEdit, $licenseEdit, $launch, $status, $heading, $body)) {
            if ($handle -eq [IntPtr]::Zero) {
                throw 'Setup window is missing an expected wizard control.'
            }
        }

        $winRect = New-Object 'ZfmSetupSmoke+Rect'
        if (-not [ZfmSetupSmoke]::GetWindowRect($window, [ref]$winRect) -or ($winRect.right - $winRect.left) -lt 740 -or ($winRect.bottom - $winRect.top) -lt 540) {
            throw 'Setup window is smaller than the redesigned wizard layout.'
        }

        $bodyRect = New-Object 'ZfmSetupSmoke+Rect'
        if (-not [ZfmSetupSmoke]::GetWindowRect($body, [ref]$bodyRect) -or ($bodyRect.bottom - $bodyRect.top) -lt 64) {
            throw 'Wizard body control is too short for a three-line description.'
        }

        # Welcome -> Folder
        if ((Get-WizardText $heading) -ne 'Welcome to ZFM' -or (Get-WizardText $next) -ne 'Next' -or [ZfmSetupSmoke]::IsWindowVisible($pathEdit)) {
            throw 'Unexpected initial wizard page state.'
        }
        Send-WizardCommand $window 102
        if ((Get-WizardText $heading) -ne 'Choose a folder' -or -not [ZfmSetupSmoke]::IsWindowVisible($pathEdit) -or -not [ZfmSetupSmoke]::IsWindowVisible($browse)) {
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

        # Reproduce an older executable held open without delete sharing.
        if ($LockThenRetry) {
            $appPath = Join-Path $TargetFolder 'zfm.exe'
            Set-Content -LiteralPath $appPath -Value 'old executable must survive until retry'
            $lockedFile = [IO.File]::Open($appPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
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
                if (-not $LockThenRetry -or $null -eq $lockedFile) {
                    throw "Interactive setup opened a modal error dialog during installation: $reason"
                }
                if ($reason -notmatch 'Close ZFM' -or $reason -notmatch 'permissions') {
                    throw "Locked file error was not actionable: $reason"
                }
                $dialog = [ZfmSetupSmoke]::FindWindowW('#32770', 'ZFM Setup')
                if ($dialog -eq [IntPtr]::Zero) { throw 'Retry dialog not found.' }
                $retry = [ZfmSetupSmoke]::GetDlgItem($dialog, 4) # IDRETRY
                if ($retry -eq [IntPtr]::Zero) { throw 'Retry button not found.' }
                $lockedFile.Dispose()
                $lockedFile = $null
                if ((Get-Content -LiteralPath $appPath -Raw).Trim() -ne 'old executable must survive until retry') {
                    throw 'Failed install changed the locked original.'
                }
                if (-not [ZfmSetupSmoke]::PostMessageW($retry, 0x00f5, [UIntPtr]::Zero, [IntPtr]::Zero)) { throw 'Could not click Retry.' }
                Start-Sleep -Milliseconds 200
                continue
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
        if ($null -ne $lockedFile) { $lockedFile.Dispose() }
        $wizardProcess.Refresh()
        if (-not $wizardProcess.HasExited) {
            Stop-Process -Id $wizardProcess.Id -Force -ErrorAction SilentlyContinue
            $wizardProcess.WaitForExit()
        }
    }
}

# Exercise a real running, different-version executable. The additional read
# handle deterministically denies replacement even on hosts that allow renaming
# a mapped executable; it is released before consenting to close the app.
function Invoke-RunningUpgrade {
    param([string]$Executable, [string]$TargetFolder, [System.Diagnostics.Process]$App, [bool]$Consent)
    $lockedFile = [IO.File]::Open((Join-Path $TargetFolder 'zfm.exe'), [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $process = Start-Process -FilePath $Executable -PassThru
    try {
        Wait-Until { $process.Refresh(); $process.MainWindowHandle -ne [IntPtr]::Zero -or $process.HasExited } 'upgrade wizard'
        if ($process.HasExited) { throw 'Upgrade wizard exited before opening.' }
        $window = $process.MainWindowHandle
        Send-WizardCommand $window 102
        Set-WizardText ([ZfmSetupSmoke]::GetDlgItem($window, 105)) $TargetFolder
        Send-WizardCommand $window 102
        Send-WizardCommand $window 102
        Wait-Until { -not [ZfmSetupSmoke]::IsWindowEnabled($window) -or $process.HasExited } 'the running-app prompt'
        if ($process.HasExited) { throw 'Upgrade wizard exited before asking permission.' }
        $dialog = [ZfmSetupSmoke]::FindWindowW('#32770', 'ZFM Setup')
        if ($dialog -eq [IntPtr]::Zero) { throw 'Running-app prompt not found.' }
        $buttonId = if ($Consent) { 6 } else { 7 } # IDYES / IDNO
        $button = [ZfmSetupSmoke]::GetDlgItem($dialog, $buttonId)
        if ($button -eq [IntPtr]::Zero) { throw 'Expected permission prompt instead of a generic retry.' }
        $lockedFile.Dispose()
        $lockedFile = $null
        if (-not [ZfmSetupSmoke]::PostMessageW($button, 0x00f5, [UIntPtr]::Zero, [IntPtr]::Zero)) { throw 'Could not answer running-app prompt.' }
        if (-not $Consent) {
            Wait-Until { [ZfmSetupSmoke]::IsWindowEnabled($window) } 'wizard to return after declining'
            $App.Refresh()
            if ($App.HasExited) { throw 'Setup closed ZFM despite the user declining.' }
            Send-WizardCommand $window 103
        } else {
            $next = [ZfmSetupSmoke]::GetDlgItem($window, 102)
            $launch = [ZfmSetupSmoke]::GetDlgItem($window, 107)
            Wait-Until { $process.Refresh(); $process.HasExited -or ([ZfmSetupSmoke]::IsWindowVisible($launch) -and (Get-WizardText $next) -eq 'Finish') } 'the upgraded installation'
            if ($process.HasExited) { throw 'Upgrade exited without reaching Finish.' }
            $App.Refresh()
            if (-not $App.HasExited) { throw 'Setup did not close the selected ZFM instance.' }
            Send-WizardCommand $window 102
        }
        if (-not $process.WaitForExit(30000)) { throw 'Upgrade wizard did not close.' }
    } finally {
        if ($null -ne $lockedFile) { $lockedFile.Dispose() }
        $process.Refresh()
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force; $process.WaitForExit() }
    }
}

try {
    Assert-SetupMetadata $setup
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
    if ($entry.Publisher -ne 'Zelvior Labs') {
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

    Assert-SetupMetadata $uninstaller

    # 4. The installed application really starts.
    & ./tests/gui-smoke-windows.ps1 -Executable (Join-Path $first 'zfm.exe')

    # 5. Running setup again over its own folder is an upgrade, not a conflict.
    $code = Invoke-Setup $setup @('/S', "/D=`"$first`"")
    Assert-ExitCode $code 0 'Reinstalling over an existing installation'
    Assert-Installed $first

    # Reinstall the same build while the installed program is running. Identical
    # binaries must not be replaced or require the user to quit ZFM.
    $runningApp = Start-Process -FilePath (Join-Path $first 'zfm.exe') -ArgumentList ('"' + $first + '"') -PassThru
    try {
        Wait-Until { $runningApp.Refresh(); $runningApp.MainWindowHandle -ne [IntPtr]::Zero -or $runningApp.HasExited } 'the installed app to start'
        if ($runningApp.HasExited) { throw 'Installed application exited unexpectedly.' }
        $code = Invoke-Setup $setup @('/S', "/D=`"$first`"")
        Assert-ExitCode $code 0 'Reinstall while ZFM is running'
        $runningApp.Refresh()
        if ($runningApp.HasExited) { throw 'Reinstall terminated the running application.' }
    } finally {
        $runningApp.Refresh()
        if (-not $runningApp.HasExited) { Stop-Process -Id $runningApp.Id -Force; $runningApp.WaitForExit() }
    }
    # A changed, running executable requires consent. Another copy in a
    # different directory must never be closed by the installer.
    $targetApp = Join-Path $first 'zfm.exe'
    $originalHash = (Get-FileHash -LiteralPath $targetApp -Algorithm SHA256).Hash
    $stream = [IO.File]::Open($targetApp, [IO.FileMode]::Append, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.WriteByte(42) } finally { $stream.Dispose() }
    New-Item -ItemType Directory -Path $second -Force | Out-Null
    $otherApp = Join-Path $second 'zfm.exe'
    Copy-Item -LiteralPath $targetApp -Destination $otherApp
    $runningApp = Start-Process -FilePath $targetApp -ArgumentList ('"' + $first + '"') -PassThru
    $otherProcess = Start-Process -FilePath $otherApp -ArgumentList ('"' + $second + '"') -PassThru
    try {
        Wait-Until { $runningApp.Refresh(); $otherProcess.Refresh(); ($runningApp.MainWindowHandle -ne [IntPtr]::Zero -and $otherProcess.MainWindowHandle -ne [IntPtr]::Zero) -or $runningApp.HasExited -or $otherProcess.HasExited } 'both application windows'
        if ($runningApp.HasExited -or $otherProcess.HasExited) { throw 'Test application exited before upgrade.' }
        Invoke-RunningUpgrade -Executable $setup -TargetFolder $first -App $runningApp -Consent $false
        $otherProcess.Refresh()
        if ($otherProcess.HasExited) { throw 'Setup closed a different ZFM installation.' }
        Invoke-RunningUpgrade -Executable $setup -TargetFolder $first -App $runningApp -Consent $true
        $otherProcess.Refresh()
        if ($otherProcess.HasExited) { throw 'Setup closed a different ZFM installation.' }
        if ((Get-FileHash -LiteralPath $targetApp -Algorithm SHA256).Hash -ne $originalHash) { throw 'Running-app upgrade did not install the bundled binary.' }
    } finally {
        foreach ($p in @($runningApp, $otherProcess)) {
            $p.Refresh()
            if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force; $p.WaitForExit() }
        }
        Remove-Item -LiteralPath $second -Recurse -Force -ErrorAction SilentlyContinue
    }

    # A changed, locked executable must remain intact until the user releases
    # it and explicitly clicks Retry; setup then completes normally.
    $originalHash = (Get-FileHash -LiteralPath (Join-Path $first 'zfm.exe') -Algorithm SHA256).Hash
    Invoke-InteractiveSetup -Executable $setup -TargetFolder $first -LockThenRetry
    if ((Get-FileHash -LiteralPath (Join-Path $first 'zfm.exe') -Algorithm SHA256).Hash -ne $originalHash) {
        throw 'Retry did not restore the bundled application.'
    }
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
} catch {
    Write-Host "::error title=Installer smoke test::$($_.Exception.Message)"
    throw
} finally {
    foreach ($folder in @($first, $second, $wizard)) {
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

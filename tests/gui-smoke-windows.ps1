#Requires -Version 5.1
param([Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference = 'Stop'
if (-not ('ZfmSmoke' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class ZfmSmoke {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int left, top, right, bottom; }
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern int GetWindowLongW(IntPtr window, int index);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern IntPtr GetDlgItem(IntPtr parent, int id);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern int GetClassNameW(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    public static extern int GetWindowTextW(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern bool PostMessageW(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern bool SetWindowTextW(IntPtr window, string text);
    [DllImport("user32.dll")]
    public static extern IntPtr SendMessageTimeoutW(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam, uint flags, uint timeout, out UIntPtr result);
}
'@
}
$work = Join-Path ([IO.Path]::GetTempPath()) ('zfm-gui-smoke-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$process = $null
try {
    Set-Content -LiteralPath (Join-Path $work 'example.txt') -Value 'GUI smoke fixture'
    Set-Content -LiteralPath (Join-Path $work 'second.txt') -Value 'Second fixture'
    $process = Start-Process -FilePath (Resolve-Path -LiteralPath $Executable).Path -ArgumentList ('"' + $work + '"') -PassThru
    $window = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $process.Refresh()
        if ($process.HasExited) { throw "ZFM exited before creating a window (exit code $($process.ExitCode))." }
        $window = $process.MainWindowHandle
        if ($window -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($window -eq [IntPtr]::Zero) { throw 'ZFM did not show a window within 10 seconds.' }
    $title = New-Object System.Text.StringBuilder 256
    [void][ZfmSmoke]::GetWindowTextW($window, $title, $title.Capacity)
    if (-not $title.ToString().StartsWith('ZFM')) { throw "Unexpected window title: $title" }
    $windowClass = New-Object System.Text.StringBuilder 256
    [void][ZfmSmoke]::GetClassNameW($window, $windowClass, $windowClass.Capacity)
    if ($windowClass.ToString() -ne 'ZFMNativeWindow') {
        throw "Expected the ZFM main window, got class '$windowClass' and title '$title' (possibly a startup error dialog)."
    }
    if (-not [ZfmSmoke]::IsWindowVisible($window)) { throw 'ZFM main window is not visible.' }

    # Identify controls by their Win32 IDs. Passing PowerShell $null to a .NET
    # string argument can become an empty string, so FindWindowEx's title filter
    # accidentally searched for an unlabelled button instead of any button.
    $controls = @(
        @{ Id = 101; Class = 'Button'; Label = 'Back' },
        @{ Id = 105; Class = 'Button'; Label = 'Go' },
        @{ Id = 106; Class = 'Edit'; Label = $null },
        @{ Id = 107; Class = 'ListBox'; Label = $null },
        @{ Id = 110; Class = 'Button'; Label = 'New tab' },
        @{ Id = 111; Class = 'Button'; Label = 'Close tab' },
        @{ Id = 112; Class = 'Edit'; Label = $null },
        @{ Id = 115; Class = 'Button'; Label = 'Show hidden' },
        @{ Id = 116; Class = 'Edit'; Label = $null },
        @{ Id = 121; Class = 'Button'; Label = 'Root' }
    )
    foreach ($control in $controls) {
        $handle = [ZfmSmoke]::GetDlgItem($window, $control.Id)
        if ($handle -eq [IntPtr]::Zero) { throw "Missing control ID $($control.Id) ($($control.Class))." }
        $actualClass = New-Object System.Text.StringBuilder 256
        [void][ZfmSmoke]::GetClassNameW($handle, $actualClass, $actualClass.Capacity)
        if ($actualClass.ToString() -ne $control.Class) {
            throw "Control ID $($control.Id) has class '$actualClass', expected '$($control.Class)'."
        }
        if (-not [ZfmSmoke]::IsWindowVisible($handle)) { throw "Control ID $($control.Id) is not visible." }
        if ($null -ne $control.Label) {
            $actualLabel = New-Object System.Text.StringBuilder 256
            [void][ZfmSmoke]::GetWindowTextW($handle, $actualLabel, $actualLabel.Capacity)
            if ($actualLabel.ToString() -ne $control.Label) { throw "Unexpected control label '$actualLabel'." }
        }
    }
    if ([ZfmSmoke]::GetDlgItem($window, 32767) -ne [IntPtr]::Zero) { throw 'Unexpected control with unused ID.' }
    $button = [ZfmSmoke]::GetDlgItem($window, 101)
    if (([ZfmSmoke]::GetWindowLongW($button, -16) -band 0x0f) -ne 0x0b) { throw 'Navigation button is not using the redesigned style.' }
    $list = [ZfmSmoke]::GetDlgItem($window, 107)
    $nameField = [ZfmSmoke]::GetDlgItem($window, 116)
    $listRect = New-Object 'ZfmSmoke+Rect'
    $nameRect = New-Object 'ZfmSmoke+Rect'
    if (-not [ZfmSmoke]::GetWindowRect($list, [ref]$listRect) -or -not [ZfmSmoke]::GetWindowRect($nameField, [ref]$nameRect)) {
        throw 'Could not inspect the layout.'
    }
    if ($listRect.top -le $nameRect.bottom -or ($listRect.bottom - $listRect.top) -lt 100) {
        throw 'File list overlaps the toolbar or has insufficient height.'
    }
    # Verify that asynchronous directory enumeration actually populates the list.
    $populated = $false
    for ($i = 0; $i -lt 100; $i++) {
        $count = [UIntPtr]::Zero
        if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x18b, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$count) -eq [IntPtr]::Zero) {
            throw 'List control did not respond.'
        }
        if ($count.ToUInt64() -eq 2) { $populated = $true; break }
        $process.Refresh()
        if ($process.HasExited) { throw 'ZFM exited while listing the directory.' }
        Start-Sleep -Milliseconds 100
    }
    if (-not $populated) { throw 'ZFM did not list the fixture file within 10 seconds.' }
    # Extended selection accepts more than one row and reports both.
    if (([ZfmSmoke]::GetWindowLongW($list, -16) -band 0x0800) -eq 0) { throw 'List does not support extended selection.' }
    foreach ($index in @(0, 1)) {
        $result = [UIntPtr]::Zero
        if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$index, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not select fixture.' }
    }
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x190, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero -or $result.ToUInt64() -ne 2) { throw 'Multiple selection failed.' }
    # A click below the rows clears the selection rather than retaining it.
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Cannot select the fixture.' }
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x201, [UIntPtr]::Zero, [IntPtr](120 -shl 16 -bor 10), 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Blank-space click did not return.' }
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x190, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Cannot read selection.' }
    if ($result.ToUInt64() -ne 0) { throw 'Blank-space click did not clear selection.' }
    # Copy and paste the first item into the same directory, without overwriting.
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Cannot select clipboard fixture.' }
    foreach ($id in @(130, 132)) {
        $result = [UIntPtr]::Zero
        if ([ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]$id, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw "Clipboard command $id did not return." }
    }
    $copy = Join-Path $work 'example copy.txt'
    for ($i = 0; $i -lt 100 -and -not (Test-Path -LiteralPath $copy); $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path -LiteralPath $copy)) { throw 'Same-folder paste did not create a copy.' }
    if ((Get-Content -LiteralPath $copy -Raw) -ne (Get-Content -LiteralPath (Join-Path $work 'example.txt') -Raw)) { throw 'Pasted data differs.' }

    # Wait for the worker completion and the refreshed directory listing, not
    # just publication of the copy, before starting another operation.
    $refreshed = $false
    for ($i = 0; $i -lt 100; $i++) {
        $count = [UIntPtr]::Zero
        [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x18b, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$count)
        if ($count.ToUInt64() -eq 3) { $refreshed = $true; break }
        Start-Sleep -Milliseconds 100
    }
    if (-not $refreshed) { throw 'Copy did not finish refreshing the list.' }
    # A posted command opens the real name dialog without blocking this process.
    if (-not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]139, [IntPtr]::Zero)) { throw 'Could not request new file dialog.' }
    $dialog = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $dialog = [ZfmSmoke]::FindWindowW('#32770', 'New file')
        if ($dialog -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($dialog -eq [IntPtr]::Zero) { throw 'New file dialog did not appear.' }
    $dialogEdit = [ZfmSmoke]::GetDlgItem($dialog, 1001)
    if ($dialogEdit -eq [IntPtr]::Zero -or -not [ZfmSmoke]::SetWindowTextW($dialogEdit, 'created.txt')) { throw 'Could not set the dialog name.' }
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($dialog, 0x111, [UIntPtr]1, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'New file dialog did not submit.' }
    for ($i = 0; $i -lt 100 -and -not (Test-Path -LiteralPath (Join-Path $work 'created.txt')); $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path -LiteralPath (Join-Path $work 'created.txt'))) {
        $notice = New-Object System.Text.StringBuilder 512
        [void][ZfmSmoke]::GetWindowTextW([ZfmSmoke]::GetDlgItem($window, 108), $notice, $notice.Capacity)
        throw "New file dialog did not create the file. Status: $notice"
    }
    # Refresh, new tab, close tab: synchronous calls exercise the message loop.
    foreach ($id in @(104, 110, 111)) {
        $result = [UIntPtr]::Zero
        if ([ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]$id, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) {
            throw "Window did not respond to command $id."
        }
    }
    Start-Sleep -Seconds 1
    $process.Refresh()
    if ($process.HasExited) { throw "ZFM exited while handling navigation commands (exit code $($process.ExitCode))." }
    Write-Host "Win32 window and basic controls detected: $title"
} catch {
    Write-Host "::error title=GUI smoke test::$($_.Exception.Message)"
    throw
} finally {
    if ($null -ne $process) {
        $process.Refresh()
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force; $process.WaitForExit() }
    }
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

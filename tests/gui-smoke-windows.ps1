#Requires -Version 5.1
param([Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class ZfmSmoke {
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern IntPtr GetDlgItem(IntPtr parent, int id);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern int GetClassNameW(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    public static extern int GetWindowTextW(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")]
    public static extern IntPtr SendMessageTimeoutW(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam, uint flags, uint timeout, out UIntPtr result);
}
'@
$work = Join-Path ([IO.Path]::GetTempPath()) ('zfm-gui-smoke-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$process = $null
try {
    Set-Content -LiteralPath (Join-Path $work 'example.txt') -Value 'GUI smoke fixture'
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
        @{ Id = 106; Class = 'Edit'; Label = $null },
        @{ Id = 107; Class = 'ListBox'; Label = $null }
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
    # Verify that asynchronous directory enumeration actually populates the list.
    $list = [ZfmSmoke]::GetDlgItem($window, 107)
    $populated = $false
    for ($i = 0; $i -lt 100; $i++) {
        $count = [UIntPtr]::Zero
        if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x18b, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$count) -eq [IntPtr]::Zero) {
            throw 'List control did not respond.'
        }
        if ($count.ToUInt64() -eq 1) { $populated = $true; break }
        $process.Refresh()
        if ($process.HasExited) { throw 'ZFM exited while listing the directory.' }
        Start-Sleep -Milliseconds 100
    }
    if (-not $populated) { throw 'ZFM did not list the fixture file within 10 seconds.' }
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
} finally {
    if ($null -ne $process) {
        $process.Refresh()
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force; $process.WaitForExit() }
    }
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

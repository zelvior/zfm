#Requires -Version 5.1
param([Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class ZfmSmoke {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string className, string title);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    public static extern int GetWindowTextW(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")]
    public static extern IntPtr SendMessageTimeoutW(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam, uint flags, uint timeout, out UIntPtr result);
}
'@
$process = Start-Process -FilePath (Resolve-Path -LiteralPath $Executable).Path -PassThru
try {
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
    foreach ($class in @('BUTTON', 'EDIT', 'LISTBOX')) {
        if ([ZfmSmoke]::FindWindowExW($window, [IntPtr]::Zero, $class, $null) -eq [IntPtr]::Zero) {
            throw "ZFM window does not contain a $class control."
        }
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
} finally {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
}

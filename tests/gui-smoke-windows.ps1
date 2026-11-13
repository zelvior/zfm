#Requires -Version 5.1
# Modified: cover the compact layout, tabs, settings and read-only preview.
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
    public static extern IntPtr SendMessageW(IntPtr window, uint message, UIntPtr wparam, string text);
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
    # The compact details view keeps navigation, filter and commands above the
    # list, rather than using a large branding header. Check at the default size.
    $backRect = New-Object 'ZfmSmoke+Rect'
    $filterRect = New-Object 'ZfmSmoke+Rect'
    if (-not [ZfmSmoke]::GetWindowRect([ZfmSmoke]::GetDlgItem($window, 101), [ref]$backRect) -or
        -not [ZfmSmoke]::GetWindowRect([ZfmSmoke]::GetDlgItem($window, 112), [ref]$filterRect)) {
        throw 'Could not inspect compact navigation and filter controls.'
    }
    if ($filterRect.top -lt $backRect.top -or $filterRect.bottom -gt $backRect.bottom) {
        throw 'Filter is not aligned with the navigation row.'
    }
    if (($listRect.top - $backRect.top) -gt 140) { throw 'File list is displaced by oversized chrome.' }
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
    # Preview the first text fixture; this command opens a modal dialog.
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Cannot select preview fixture.' }
    if (-not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]142, [IntPtr]::Zero)) { throw 'Could not request preview.' }
    $preview = [IntPtr]::Zero
    $previewTitle = 'ZFM Preview — ' + (Join-Path $work 'example.txt')
    for ($i = 0; $i -lt 100; $i++) {
        $preview = [ZfmSmoke]::FindWindowW('#32770', $previewTitle)
        if ($preview -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($preview -eq [IntPtr]::Zero) { throw 'Text preview did not appear.' }
    $previewEdit = [ZfmSmoke]::GetDlgItem($preview, 1001)
    if ($previewEdit -eq [IntPtr]::Zero -or (([ZfmSmoke]::GetWindowLongW($previewEdit, -16) -band 0x0800) -eq 0)) { throw 'Preview is not read-only.' }
    $previewText = New-Object System.Text.StringBuilder 256
    [void][ZfmSmoke]::GetWindowTextW($previewEdit, $previewText, $previewText.Capacity)
    if ($previewText.ToString() -notmatch 'GUI smoke fixture') { throw 'Preview text is incorrect.' }
    if (-not [ZfmSmoke]::PostMessageW($preview, 0x10, [UIntPtr]::Zero, [IntPtr]::Zero)) { throw 'Could not close preview.' }
    # Clear the selection after the preview.
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$result)
    # Settings is a modal session-only dialog; cancel must not apply changes.
    if (-not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]143, [IntPtr]::Zero)) { throw 'Could not request settings.' }
    $settings = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $settings = [ZfmSmoke]::FindWindowW('#32770', 'ZFM Settings')
        if ($settings -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($settings -eq [IntPtr]::Zero) { throw 'Settings dialog did not appear.' }
    if ([ZfmSmoke]::GetDlgItem($settings, 1001) -eq [IntPtr]::Zero -or [ZfmSmoke]::GetDlgItem($settings, 1002) -eq [IntPtr]::Zero) { throw 'Settings controls are missing.' }
    if (-not [ZfmSmoke]::PostMessageW($settings, 0x10, [UIntPtr]::Zero, [IntPtr]::Zero)) { throw 'Could not close settings.' }
    # Extended selection accepts more than one row and reports both.
    if (([ZfmSmoke]::GetWindowLongW($list, -16) -band 0x0800) -eq 0) { throw 'List does not support extended selection.' }
    foreach ($index in @(0, 1)) {
        $result = [UIntPtr]::Zero
        if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$index, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not select fixture.' }
    }
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x190, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero -or $result.ToUInt64() -ne 2) { throw 'Multiple selection failed.' }
    # Right-button press on an already-selected row preserves the group.
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x204, [UIntPtr]::Zero, [IntPtr](10 -shl 16 -bor 10), 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Right-button press did not return.' }
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x190, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero -or $result.ToUInt64() -ne 2) { throw 'Right-click did not preserve multiple selection.' }
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
    $dialogEdit = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $dialogEdit = [ZfmSmoke]::GetDlgItem($dialog, 1001)
        if ($dialogEdit -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($dialogEdit -eq [IntPtr]::Zero -or [ZfmSmoke]::SendMessageW($dialogEdit, 0x000c, [UIntPtr]::Zero, 'created.txt') -eq [IntPtr]::Zero) { throw 'Could not set the dialog name.' }
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
    # Exercise the built-in converter through the real Win32 selection and dialog.
    # Keep fixtures in the GUI directory and wait for each async refresh.
    [IO.File]::WriteAllText((Join-Path $work 'source.csv'), "name`nZelvior`n", [Text.UTF8Encoding]::new($false))
    # 2x2, 24-bit uncompressed bottom-up BMP with asymmetric pixels and row padding.
    $bmp = [byte[]]::new(70)
    $bmp[0] = 66; $bmp[1] = 77
    [BitConverter]::GetBytes([uint32]70).CopyTo($bmp, 2)
    [BitConverter]::GetBytes([uint32]54).CopyTo($bmp, 10)
    [BitConverter]::GetBytes([uint32]40).CopyTo($bmp, 14)
    [BitConverter]::GetBytes([uint32]2).CopyTo($bmp, 18)
    [BitConverter]::GetBytes([uint32]2).CopyTo($bmp, 22)
    [BitConverter]::GetBytes([uint16]1).CopyTo($bmp, 26)
    [BitConverter]::GetBytes([uint16]24).CopyTo($bmp, 28)
    [BitConverter]::GetBytes([uint32]16).CopyTo($bmp, 34)
    [byte[]]$pixels = @(255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0)
    $pixels.CopyTo($bmp, 54)
    [IO.File]::WriteAllBytes((Join-Path $work 'source.bmp'), $bmp)
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]104, [IntPtr]::Zero, 2, 3000, [ref]$result)
    function Convert-Fixture([string]$inputName, [string]$dialogTitle, [string]$outputName) {
        $index = -1
        for ($i = 0; $i -lt 100; $i++) {
            $index = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, $inputName).ToInt64() # LB_FINDSTRINGEXACT
            if ($index -ge 0) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($index -lt 0) { throw "Converter input $inputName is not in the list." }
        $result = [UIntPtr]::Zero
        [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$result)
        [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$index, 2, 3000, [ref]$result)
        if (-not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]141, [IntPtr]::Zero)) { throw 'Could not invoke converter.' }
        $dialog = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $dialog = [ZfmSmoke]::FindWindowW('#32770', $dialogTitle)
            if ($dialog -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($dialog -eq [IntPtr]::Zero) { throw "Converter dialog $dialogTitle did not appear." }
        $edit = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $edit = [ZfmSmoke]::GetDlgItem($dialog, 1001)
            if ($edit -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($edit -eq [IntPtr]::Zero -or [ZfmSmoke]::SendMessageW($edit, 0x000c, [UIntPtr]::Zero, $outputName) -eq [IntPtr]::Zero) { throw "Could not set converter output name." }
        [void][ZfmSmoke]::SendMessageTimeoutW($dialog, 0x111, [UIntPtr]1, [IntPtr]::Zero, 2, 3000, [ref]$result)
        $destination = Join-Path $work $outputName
        for ($i = 0; $i -lt 100 -and -not (Test-Path -LiteralPath $destination); $i++) { Start-Sleep -Milliseconds 100 }
        if (-not (Test-Path -LiteralPath $destination)) { throw "Converter did not create $outputName." }
        # Publication precedes the asynchronous listing refresh.
        for ($i = 0; $i -lt 100; $i++) {
            if ([ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, $outputName).ToInt64() -ge 0) { return }
            Start-Sleep -Milliseconds 100
        }
        throw "Converted file $outputName was not listed."
    }
    Convert-Fixture 'example.txt' 'Convert text to HTML' 'example.html'
    if ((Get-Content -Raw -LiteralPath (Join-Path $work 'example.html')) -notmatch '<pre>GUI smoke fixture') { throw 'Incorrect HTML conversion.' }
    Convert-Fixture 'source.csv' 'Convert CSV to JSON' 'source.json'
    if (((Get-Content -Raw -LiteralPath (Join-Path $work 'source.json')) | ConvertFrom-Json)[0].name -ne 'Zelvior') { throw 'Incorrect CSV to JSON conversion.' }
    Convert-Fixture 'source.json' 'Convert JSON to CSV' 'roundtrip.csv'
    if ((Get-Content -Raw -LiteralPath (Join-Path $work 'roundtrip.csv')) -notmatch 'Zelvior') { throw 'Incorrect JSON to CSV conversion.' }
    Convert-Fixture 'source.bmp' 'Convert BMP to ICO' 'image.ico'
    Add-Type -AssemblyName System.Drawing
    $icon = [Drawing.Icon]::new((Join-Path $work 'image.ico'), [Drawing.Size]::new(2, 2))
    try {
        $decoded = $icon.ToBitmap()
        try {
            if ($decoded.Width -ne 2 -or $decoded.Height -ne 2 -or $decoded.GetPixel(0,0).ToArgb() -ne [Drawing.Color]::Red.ToArgb()) { throw 'Windows could not correctly decode the generated icon.' }
        } finally { $decoded.Dispose() }
    } finally { $icon.Dispose() }
    Convert-Fixture 'image.ico' 'Convert ICO to BMP' 'roundtrip.bmp'
    $decoded = [Drawing.Bitmap]::new((Join-Path $work 'roundtrip.bmp'))
    try {
        if ($decoded.Width -ne 2 -or $decoded.Height -ne 2 -or $decoded.GetPixel(0,0).ToArgb() -ne [Drawing.Color]::Red.ToArgb()) { throw 'Windows could not correctly decode the generated bitmap.' }
    } finally { $decoded.Dispose() }
    # Copy a folder and a file together through the real clipboard.
    $folder = Join-Path $work 'folder-fixture'
    New-Item -ItemType Directory -Path (Join-Path $folder 'nested\empty') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $folder 'nested\file.txt'), 'nested content')
    [IO.File]::WriteAllText((Join-Path $work 'folder-companion.txt'), 'companion content')
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]104, [IntPtr]::Zero, 2, 3000, [ref]$result)
    $index = -1
    for ($i = 0; $i -lt 100; $i++) {
        $index = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'folder-fixture').ToInt64()
        if ($index -ge 0) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($index -lt 0) { throw 'Folder fixture was not listed.' }
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$result)
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$index, 2, 3000, [ref]$result)
    $secondIndex = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'folder-companion.txt').ToInt64()
    if ($secondIndex -lt 0) { throw 'Companion fixture was not listed.' }
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$secondIndex, 2, 3000, [ref]$result)
    foreach ($id in @(130, 132)) {
        if ([ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]$id, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw "Multi-item clipboard command $id did not return." }
    }
    $copiedFolder = Join-Path $work 'folder-fixture copy'
    $copiedCompanion = Join-Path $work 'folder-companion copy.txt'
    for ($i = 0; $i -lt 100; $i++) {
        if ((Test-Path -LiteralPath (Join-Path $copiedFolder 'nested\file.txt')) -and (Test-Path -LiteralPath $copiedCompanion)) { break }
        Start-Sleep -Milliseconds 100
    }
    if (-not (Test-Path -LiteralPath (Join-Path $copiedFolder 'nested\empty')) -or
        -not (Test-Path -LiteralPath (Join-Path $copiedFolder 'nested\file.txt')) -or
        [IO.File]::ReadAllText((Join-Path $copiedFolder 'nested\file.txt')) -ne 'nested content' -or
        -not (Test-Path -LiteralPath $copiedCompanion) -or
        [IO.File]::ReadAllText($copiedCompanion) -ne 'companion content') { throw 'Multi-item copy was incomplete.' }
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

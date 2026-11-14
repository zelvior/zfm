#Requires -Version 5.1
# Modified: isolate persistent settings and cover layout, tabs, settings and preview.
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
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="FindWindowW", ExactSpelling=true)]
    public static extern IntPtr FindWindowAnyTitle(string cls, IntPtr title);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string cls, string title);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern bool IsClipboardFormatAvailable(uint format);
    [DllImport("user32.dll", ExactSpelling=true)]
    public static extern bool PostMessageW(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern IntPtr SendMessageW(IntPtr window, uint message, UIntPtr wparam, string text);
    [DllImport("user32.dll")]
    public static extern IntPtr SendMessageTimeoutW(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam, uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageTimeoutW", ExactSpelling=true)]
    public static extern IntPtr GetControlText(IntPtr window, uint message, UIntPtr wparam, StringBuilder text, uint flags, uint timeout, out UIntPtr result);
}
'@
}
$work = Join-Path ([IO.Path]::GetTempPath()) ('zfm-gui-smoke-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$process = $null
$originalAppData = $env:APPDATA
$configRoot = Join-Path ([IO.Path]::GetTempPath()) ('zfm-config-smoke-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $configRoot | Out-Null
$env:APPDATA = $configRoot
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

    # A top-level window can exist before its initial listing is ready on a
    # busy runner. Wait for the intended folder before sending commands.
    $startupReady = $false
    for ($i = 0; $i -lt 200; $i++) {
        $status = New-Object System.Text.StringBuilder 2048
        $statusHandle = [ZfmSmoke]::GetDlgItem($window, 108)
        if ($statusHandle -ne [IntPtr]::Zero) { [void][ZfmSmoke]::GetWindowTextW($statusHandle, $status, $status.Capacity) }
        if ($status.ToString().EndsWith("  |  $work") -and [ZfmSmoke]::GetDlgItem($window, 107) -ne [IntPtr]::Zero) { $startupReady = $true; break }
        $process.Refresh()
        if ($process.HasExited) { throw 'ZFM exited during startup.' }
        Start-Sleep -Milliseconds 100
    }
    if (-not $startupReady) { throw "Initial listing was not ready; status=$status" }

    # Bookmark the current folder and ensure adding it twice is idempotent.
    $result = [UIntPtr]::Zero
    foreach ($attempt in @(1, 2)) {
        if ([ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]167, [IntPtr]::Zero, 2, 10000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not bookmark folder.' }
    }
    $bookmarkFile = Join-Path $configRoot 'zfm\bookmarks.conf'
    $bookmarkHex = [BitConverter]::ToString([Text.Encoding]::UTF8.GetBytes($work)).Replace('-', '').ToLowerInvariant()
    if (-not (Test-Path -LiteralPath $bookmarkFile) -or ((Get-Content -LiteralPath $bookmarkFile) -join "`n") -ne ("version=1`n" + $bookmarkHex)) { throw 'Bookmark was not saved correctly.' }

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
        @{ Id = 121; Class = 'Button'; Label = 'Root' },
        @{ Id = 172; Class = 'Button'; Label = 'Actions ▾' }
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
    # The visible Actions button opens the same file-list command menu.
    $actions = [ZfmSmoke]::GetDlgItem($window, 172)
    [void][ZfmSmoke]::PostMessageW($actions, 0x00f5, [UIntPtr]::Zero, [IntPtr]::Zero) # BM_CLICK
    $popup = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $popup = [ZfmSmoke]::FindWindowAnyTitle('#32768', [IntPtr]::Zero)
        $ownerPid = [uint32]0
        if ($popup -ne [IntPtr]::Zero) { [void][ZfmSmoke]::GetWindowThreadProcessId($popup, [ref]$ownerPid) }
        if ($popup -ne [IntPtr]::Zero -and $ownerPid -eq $process.Id -and [ZfmSmoke]::IsWindowVisible($popup)) { break }
        $popup = [IntPtr]::Zero
        Start-Sleep -Milliseconds 100
    }
    if ($popup -eq [IntPtr]::Zero) { throw 'Actions menu did not open.' }
    [void][ZfmSmoke]::PostMessageW($popup, 0x100, [UIntPtr]27, [IntPtr]::Zero) # Escape
    for ($i = 0; $i -lt 100 -and [ZfmSmoke]::IsWindowVisible($popup); $i++) { Start-Sleep -Milliseconds 100 }
    if ([ZfmSmoke]::IsWindowVisible($popup)) { throw 'Actions menu did not close.' }
    # External changes appear without pressing Reload; selection survives the update.
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]::Zero, 2, 3000, [ref]$result)
    $liveFixture = Join-Path $work 'live-external.txt'
    [IO.File]::WriteAllText($liveFixture, 'external')
    $appeared = $false
    for ($i = 0; $i -lt 150; $i++) {
        $index = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'live-external.txt').ToInt64()
        if ($index -ge 0) { $appeared = $true; break }
        Start-Sleep -Milliseconds 100
    }
    if (-not $appeared) { throw 'Automatic folder refresh did not detect an external file.' }
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x190, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result)
    if ($result.ToUInt64() -ne 1) { throw 'Automatic refresh lost the selection.' }
    Remove-Item -LiteralPath $liveFixture
    $removed = $false
    for ($i = 0; $i -lt 150; $i++) {
        if ([ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'live-external.txt').ToInt64() -lt 0) { $removed = $true; break }
        Start-Sleep -Milliseconds 100
    }
    if (-not $removed) { throw 'Automatic folder refresh did not detect a removal.' }
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$result)
    # Preview the first text fixture; this command opens a modal dialog.
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Cannot select preview fixture.' }
    # Open the selection-aware Actions menu and choose its Preview entry.
    [void][ZfmSmoke]::PostMessageW($actions, 0x00f5, [UIntPtr]::Zero, [IntPtr]::Zero)
    $popup = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $popup = [ZfmSmoke]::FindWindowAnyTitle('#32768', [IntPtr]::Zero)
        $ownerPid = [uint32]0
        if ($popup -ne [IntPtr]::Zero) { [void][ZfmSmoke]::GetWindowThreadProcessId($popup, [ref]$ownerPid) }
        if ($popup -ne [IntPtr]::Zero -and $ownerPid -eq $process.Id -and [ZfmSmoke]::IsWindowVisible($popup)) { break }
        $popup = [IntPtr]::Zero
        Start-Sleep -Milliseconds 100
    }
    if ($popup -eq [IntPtr]::Zero) { throw 'Selected-item Actions menu did not open.' }
    [void][ZfmSmoke]::PostMessageW($popup, 0x100, [UIntPtr]40, [IntPtr]::Zero) # Open
    [void][ZfmSmoke]::PostMessageW($popup, 0x100, [UIntPtr]40, [IntPtr]::Zero) # Preview
    [void][ZfmSmoke]::PostMessageW($popup, 0x100, [UIntPtr]13, [IntPtr]::Zero) # Enter
    $preview = [IntPtr]::Zero
    $previewTitle = 'ZFM Preview — ' + (Join-Path $work 'example.txt')
    for ($i = 0; $i -lt 100; $i++) {
        $preview = [ZfmSmoke]::FindWindowW('#32770', $previewTitle)
        if ($preview -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($preview -eq [IntPtr]::Zero) { throw 'Text preview did not appear.' }
    $previewEdit = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $previewEdit = [ZfmSmoke]::GetDlgItem($preview, 1001)
        if ($previewEdit -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($previewEdit -eq [IntPtr]::Zero -or (([ZfmSmoke]::GetWindowLongW($previewEdit, -16) -band 0x0800) -eq 0)) { throw 'Preview is not read-only.' }
    $previewText = New-Object System.Text.StringBuilder 256
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::GetControlText($previewEdit, 0x000D, [UIntPtr]$previewText.Capacity, $previewText, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not read preview text.' }
    if ($previewText.ToString() -notmatch 'GUI smoke fixture') { throw "Preview text is incorrect: $previewText" }
    if (-not [ZfmSmoke]::PostMessageW($preview, 0x10, [UIntPtr]::Zero, [IntPtr]::Zero)) { throw 'Could not close preview.' }
    # Hash the still-selected fixture and check the actual displayed digest.
    if (-not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]183, [IntPtr]::Zero)) { throw 'Could not request file hashes.' }
    $hashDialog = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $hashDialog = [ZfmSmoke]::FindWindowW('#32770', 'File hashes')
        if ($hashDialog -ne [IntPtr]::Zero -and [ZfmSmoke]::GetDlgItem($hashDialog, 1002) -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($hashDialog -eq [IntPtr]::Zero) { throw 'File hash report did not appear.' }
    $hashText = New-Object System.Text.StringBuilder 4096
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::GetControlText([ZfmSmoke]::GetDlgItem($hashDialog, 1002), 0x000D, [UIntPtr]$hashText.Capacity, $hashText, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not read file hash report.' }
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $expected = [BitConverter]::ToString($sha.ComputeHash([IO.File]::ReadAllBytes((Join-Path $work 'example.txt')))).Replace('-', '').ToLowerInvariant() } finally { $sha.Dispose() }
    if (-not $hashText.ToString().Contains($expected)) { throw "File hash report did not contain the expected SHA-256: $hashText" }
    if (-not [ZfmSmoke]::PostMessageW($hashDialog, 0x10, [UIntPtr]::Zero, [IntPtr]::Zero)) { throw 'Could not close file hash report.' }
    # The tag database is searchable from Actions without altering the file.
    [void][ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]182, [IntPtr]::Zero)
    $tagEdit = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $tagEdit = [ZfmSmoke]::FindWindowW('#32770', 'Edit tags — comma-separated; empty clears')
        if ($tagEdit -ne [IntPtr]::Zero -and [ZfmSmoke]::GetDlgItem($tagEdit, 1001) -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($tagEdit -eq [IntPtr]::Zero) { throw 'Tag editor did not appear.' }
    [void][ZfmSmoke]::SendMessageW([ZfmSmoke]::GetDlgItem($tagEdit, 1001), 0x000c, [UIntPtr]::Zero, 'smoke-tag')
    [void][ZfmSmoke]::PostMessageW($tagEdit, 0x111, [UIntPtr]1, [IntPtr]::Zero)
    $tagConfig = Join-Path $configRoot 'zfm\tags.conf'
    for ($i = 0; $i -lt 100 -and -not (Test-Path -LiteralPath $tagConfig); $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path -LiteralPath $tagConfig)) { throw 'Tags were not saved.' }
    [void][ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]191, [IntPtr]::Zero)
    $tagSearch = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $tagSearch = [ZfmSmoke]::FindWindowW('#32770', 'Find exact case-sensitive tag')
        if ($tagSearch -ne [IntPtr]::Zero -and [ZfmSmoke]::GetDlgItem($tagSearch, 1001) -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($tagSearch -eq [IntPtr]::Zero) { throw 'Tag search did not appear.' }
    [void][ZfmSmoke]::SendMessageW([ZfmSmoke]::GetDlgItem($tagSearch, 1001), 0x000c, [UIntPtr]::Zero, 'smoke-tag')
    [void][ZfmSmoke]::PostMessageW($tagSearch, 0x111, [UIntPtr]1, [IntPtr]::Zero)
    $tagReport = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $tagReport = [ZfmSmoke]::FindWindowW('#32770', 'Saved paths by tag')
        if ($tagReport -ne [IntPtr]::Zero -and [ZfmSmoke]::GetDlgItem($tagReport, 1002) -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($tagReport -eq [IntPtr]::Zero) { throw 'Tag report did not appear.' }
    $tagText = New-Object System.Text.StringBuilder 4096
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::GetControlText([ZfmSmoke]::GetDlgItem($tagReport, 1002), 0x000d, [UIntPtr]$tagText.Capacity, $tagText, 2, 3000, [ref]$result)
    if (-not $tagText.ToString().Contains((Join-Path $work 'example.txt'))) { throw "Tag search omitted the fixture: $tagText" }
    [void][ZfmSmoke]::PostMessageW($tagReport, 0x10, [UIntPtr]::Zero, [IntPtr]::Zero)
    for ($i = 0; $i -lt 100 -and [ZfmSmoke]::IsWindowVisible($tagReport); $i++) { Start-Sleep -Milliseconds 100 }
    # Reach the multiline editor and cancel at the final confirmation: no code runs.
    [void][ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]188, [IntPtr]::Zero)
    foreach ($title in @('Interpreter executable (e.g. python, node, cmd.exe)', 'Single interpreter argument (e.g. /C, -c, -e, -File)')) {
        $prompt = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $prompt = [ZfmSmoke]::FindWindowW('#32770', $title)
            if ($prompt -ne [IntPtr]::Zero -and [ZfmSmoke]::GetDlgItem($prompt, 1001) -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($prompt -eq [IntPtr]::Zero) { throw "Interpreter prompt not shown: $title" }
        [void][ZfmSmoke]::PostMessageW($prompt, 0x111, [UIntPtr]1, [IntPtr]::Zero)
    }
    $snippet = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $snippet = [ZfmSmoke]::FindWindowW('#32770', 'Code snippet')
        if ($snippet -ne [IntPtr]::Zero -and [ZfmSmoke]::GetDlgItem($snippet, 1001) -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($snippet -eq [IntPtr]::Zero) { throw 'Snippet editor not shown.' }
    $snippetControl = [ZfmSmoke]::GetDlgItem($snippet, 1001)
    if (([ZfmSmoke]::GetWindowLongW($snippetControl, -16) -band 0x0004) -eq 0) { throw 'Snippet editor is not multiline.' }
    [void][ZfmSmoke]::SendMessageW($snippetControl, 0x000c, [UIntPtr]::Zero, "echo first`r`necho second")
    $snippetText = New-Object System.Text.StringBuilder 256
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::GetControlText($snippetControl, 0x000d, [UIntPtr]$snippetText.Capacity, $snippetText, 2, 3000, [ref]$result)
    if ($snippetText.ToString() -notmatch 'first\s+echo second') { throw 'Snippet editor lost its second line.' }
    [void][ZfmSmoke]::PostMessageW($snippet, 0x111, [UIntPtr]1, [IntPtr]::Zero)
    $confirmRun = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $confirmRun = [ZfmSmoke]::FindWindowW('#32770', 'Run external code?')
        if ($confirmRun -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($confirmRun -eq [IntPtr]::Zero) { throw 'Run confirmation not shown.' }
    [void][ZfmSmoke]::PostMessageW($confirmRun, 0x111, [UIntPtr]2, [IntPtr]::Zero)
    for ($i = 0; $i -lt 100 -and [ZfmSmoke]::IsWindowVisible($confirmRun); $i++) { Start-Sleep -Milliseconds 100 }
    # Clear the selection after the preview.
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$result)
    # Settings is a modal dialog; cancel must not apply changes.
    if (-not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]143, [IntPtr]::Zero)) { throw 'Could not request settings.' }
    $settings = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $settings = [ZfmSmoke]::FindWindowW('#32770', 'ZFM Settings')
        if ($settings -ne [IntPtr]::Zero -and [ZfmSmoke]::GetDlgItem($settings, 1007) -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($settings -eq [IntPtr]::Zero) { throw 'Settings dialog did not appear.' }
    foreach ($id in @(1001, 1002, 1004, 1005, 1006, 1007)) {
        if ([ZfmSmoke]::GetDlgItem($settings, $id) -eq [IntPtr]::Zero) { throw "Settings control $id is missing." }
    }
    # All controls must remain inside the dialog and the rows must not overlap.
    $settingsRect = New-Object 'ZfmSmoke+Rect'
    if (-not [ZfmSmoke]::GetWindowRect($settings, [ref]$settingsRect)) { throw 'Cannot inspect settings dialog bounds.' }
    $rowRects = @()
    foreach ($id in @(1010, 1001, 1002, 1004, 1005, 1011, 1006, 1007, 1003, 1, 2)) {
        $rect = New-Object 'ZfmSmoke+Rect'
        if (-not [ZfmSmoke]::GetWindowRect([ZfmSmoke]::GetDlgItem($settings, $id), [ref]$rect)) { throw "Cannot inspect settings control $id." }
        if ($rect.left -lt $settingsRect.left -or $rect.right -gt $settingsRect.right -or $rect.top -lt $settingsRect.top -or $rect.bottom -gt $settingsRect.bottom) { throw "Settings control $id is clipped." }
        if ($id -ne 2) { $rowRects += $rect }
    }
    for ($i = 1; $i -lt $rowRects.Count; $i++) {
        if ($rowRects[$i].top -lt $rowRects[$i-1].bottom) { throw "Settings rows overlap at index $i." }
    }
    # The custom-painted switches retain native checkbox state and activation.
    $switch = [ZfmSmoke]::GetDlgItem($settings, 1001)
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($switch, 0x00f0, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not read settings switch.' }
    $before = $result.ToUInt64()
    if ([ZfmSmoke]::SendMessageTimeoutW($switch, 0x00f5, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not activate settings switch.' }
    if ([ZfmSmoke]::SendMessageTimeoutW($switch, 0x00f0, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero -or $result.ToUInt64() -eq $before) { throw 'Settings switch did not toggle.' }
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($settings, 0x10, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not cancel settings.' }
    $configFile = Join-Path $configRoot 'zfm\settings.conf'
    if (Test-Path -LiteralPath $configFile) { throw 'Cancelling settings created a file.' }
    # Extended selection accepts more than one row and reports both.
    if (([ZfmSmoke]::GetWindowLongW($list, -16) -band 0x0800) -eq 0) { throw 'List does not support extended selection.' }
    foreach ($index in @(0, 1)) {
        $result = [UIntPtr]::Zero
        if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$index, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not select fixture.' }
    }
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x190, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero -or $result.ToUInt64() -ne 2) { throw 'Multiple selection failed.' }
    # Add and then remove a tag on both selected paths, retaining existing tags.
    foreach ($operation in @(
        @{ Id = 192; Title = 'Add tags to 2 selected path(s) — comma-separated'; Count = 2 },
        @{ Id = 193; Title = 'Remove tags from 2 selected path(s) — comma-separated'; Count = 0 }
    )) {
        [void][ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]$operation.Id, [IntPtr]::Zero)
        $batchTag = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $batchTag = [ZfmSmoke]::FindWindowW('#32770', $operation.Title)
            if ($batchTag -ne [IntPtr]::Zero -and [ZfmSmoke]::GetDlgItem($batchTag, 1001) -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($batchTag -eq [IntPtr]::Zero) { throw "Batch tag prompt did not open: $($operation.Title)" }
        [void][ZfmSmoke]::SendMessageW([ZfmSmoke]::GetDlgItem($batchTag, 1001), 0x000c, [UIntPtr]::Zero, 'batch-tag')
        [void][ZfmSmoke]::PostMessageW($batchTag, 0x111, [UIntPtr]1, [IntPtr]::Zero)
        $saved = $false
        for ($i = 0; $i -lt 100; $i++) {
            $db = Get-Content -LiteralPath $tagConfig -Raw
            $count = ([regex]::Matches($db, '62617463682d746167')).Count # hex-encoded batch-tag
            if ($count -eq $operation.Count -and $db.Contains('736d6f6b652d746167')) { $saved = $true; break }
            Start-Sleep -Milliseconds 100
        }
        if (-not $saved) { throw "Batch tag operation did not persist correctly: $($operation.Title)" }
    }
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
    # Mono, 16-bit, 8 kHz PCM with two samples.
    [byte[]]$wav = @(82,73,70,70,40,0,0,0,87,65,86,69,102,109,116,32,16,0,0,0,1,0,1,0,64,31,0,0,128,62,0,0,2,0,16,0,100,97,116,97,4,0,0,0,0,128,255,127)
    [IO.File]::WriteAllBytes((Join-Path $work 'source-audio.wav'), $wav)
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]104, [IntPtr]::Zero, 2, 3000, [ref]$result)
    function Convert-Fixture([string]$inputName, [string]$dialogTitle, [string]$outputName, [int]$command = 141) {
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
        if (-not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]$command, [IntPtr]::Zero)) { throw 'Could not invoke converter.' }
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
    Convert-Fixture 'source-audio.wav' 'Convert PCM WAV to AIFF (metadata omitted)' 'source-audio.aiff'
    Convert-Fixture 'source-audio.aiff' 'Convert PCM AIFF to WAV (metadata omitted)' 'roundtrip-audio.wav'
    Convert-Fixture 'source-audio.wav' 'Convert PCM WAV to AU (metadata omitted)' 'source-audio.au' 170
    Convert-Fixture 'source-audio.au' 'Convert PCM AU to WAV (metadata omitted)' 'roundtrip-au.wav'
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes((Join-Path $work 'roundtrip-au.wav'))) -ne [Convert]::ToBase64String($wav)) { throw 'AU conversion round trip differs.' }
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes((Join-Path $work 'roundtrip-audio.wav'))) -ne [Convert]::ToBase64String($wav)) { throw 'Audio conversion round trip differs.' }
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
    # Exercise the direct Copy to / Move to commands and their destination prompts.
    $destination = Join-Path $configRoot 'destination'
    New-Item -ItemType Directory -Path $destination | Out-Null
    $folderIndex = -1; $companionIndex = -1
    for ($i = 0; $i -lt 100; $i++) {
        $folderIndex = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'folder-fixture').ToInt64()
        $companionIndex = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'folder-companion.txt').ToInt64()
        $copyIndex = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'folder-fixture copy').ToInt64()
        if ($folderIndex -ge 0 -and $companionIndex -ge 0 -and $copyIndex -ge 0) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($folderIndex -lt 0 -or $companionIndex -lt 0 -or $copyIndex -lt 0) { throw 'List did not refresh after clipboard copy.' }
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$result)
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$folderIndex, 2, 3000, [ref]$result)
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$companionIndex, 2, 3000, [ref]$result)
    function Submit-Destination([int]$command, [string]$title, [string]$destination) {
        if (-not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]$command, [IntPtr]::Zero)) { throw "Could not request $title." }
        $dialog = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $dialog = [ZfmSmoke]::FindWindowW('#32770', $title)
            if ($dialog -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($dialog -eq [IntPtr]::Zero) { throw "$title dialog did not appear." }
        $edit = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $edit = [ZfmSmoke]::GetDlgItem($dialog, 1001)
            if ($edit -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($edit -eq [IntPtr]::Zero -or [ZfmSmoke]::SendMessageW($edit, 0x000c, [UIntPtr]::Zero, $destination) -eq [IntPtr]::Zero) { throw "Could not set $title destination." }
        if (-not [ZfmSmoke]::PostMessageW($dialog, 0x111, [UIntPtr]1, [IntPtr]::Zero)) { throw "Could not submit $title." }
    }
    Submit-Destination 164 'Copy to existing folder' $destination
    for ($i = 0; $i -lt 100; $i++) {
        if ((Test-Path -LiteralPath (Join-Path $destination 'folder-fixture\nested\file.txt')) -and (Test-Path -LiteralPath (Join-Path $destination 'folder-companion.txt'))) { break }
        Start-Sleep -Milliseconds 100
    }
    if ([IO.File]::ReadAllText((Join-Path $destination 'folder-fixture\nested\file.txt')) -ne 'nested content' -or
        [IO.File]::ReadAllText((Join-Path $destination 'folder-companion.txt')) -ne 'companion content') { throw 'Direct multi-item copy failed.' }
    # Wait for completion and the refreshed list before selecting both move sources.
    $companionIndex = -1; $folderIndex = -1
    for ($i = 0; $i -lt 100; $i++) {
        $status = New-Object System.Text.StringBuilder 512
        [void][ZfmSmoke]::GetWindowTextW([ZfmSmoke]::GetDlgItem($window, 108), $status, $status.Capacity)
        if ($status.ToString() -match 'Copied 2 items') {
            $companionIndex = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'folder-companion.txt').ToInt64()
            $folderIndex = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'folder-fixture').ToInt64()
            if ($companionIndex -ge 0 -and $folderIndex -ge 0) { break }
        }
        Start-Sleep -Milliseconds 100
    }
    if ($companionIndex -lt 0 -or $folderIndex -lt 0) { throw 'Direct copy did not finish.' }
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$result)
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$companionIndex, 2, 3000, [ref]$result)
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$folderIndex, 2, 3000, [ref]$result)
    $moveDestination = Join-Path $configRoot 'move-destination'
    New-Item -ItemType Directory -Path $moveDestination | Out-Null
    Submit-Destination 165 'Move to existing folder' $moveDestination
    for ($i = 0; $i -lt 100; $i++) {
        if ((Test-Path -LiteralPath (Join-Path $moveDestination 'folder-companion.txt')) -and (Test-Path -LiteralPath (Join-Path $moveDestination 'folder-fixture\nested\file.txt'))) { break }
        Start-Sleep -Milliseconds 100
    }
    if ([IO.File]::ReadAllText((Join-Path $moveDestination 'folder-companion.txt')) -ne 'companion content' -or
        [IO.File]::ReadAllText((Join-Path $moveDestination 'folder-fixture\nested\file.txt')) -ne 'nested content' -or
        (Test-Path -LiteralPath (Join-Path $work 'folder-companion.txt')) -or (Test-Path -LiteralPath (Join-Path $work 'folder-fixture'))) { throw 'Direct multi-item move failed.' }

    # Test actual archive commands with the engine placed beside the binary by CI.
    $engine = Join-Path (Split-Path -Parent (Resolve-Path -LiteralPath $Executable).Path) 'zfm-archive-engine.exe'
    if (Test-Path -LiteralPath $engine) {
        $index = -1
        for ($i = 0; $i -lt 100; $i++) {
            $status = New-Object System.Text.StringBuilder 512
            [void][ZfmSmoke]::GetWindowTextW([ZfmSmoke]::GetDlgItem($window, 108), $status, $status.Capacity)
            if ($status.ToString() -match 'Moved 2 items') {
                $index = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'example.txt').ToInt64()
                if ($index -ge 0) { break }
            }
            Start-Sleep -Milliseconds 100
        }
        if ($index -lt 0) { throw 'Move did not finish before archive test.' }
        [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$result)
        [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$index, 2, 3000, [ref]$result)
        Submit-Destination 151 'Create .7z, .zip, .tar, compressed TAR, .gz, .bz2 or .xz' 'gui-test.zip'
        $archive = Join-Path $work 'gui-test.zip'
        for ($i = 0; $i -lt 200 -and -not (Test-Path -LiteralPath $archive); $i++) { Start-Sleep -Milliseconds 100 }
        if (-not (Test-Path -LiteralPath $archive)) { throw 'Archive creation failed.' }
        $archiveIndex = -1
        for ($i = 0; $i -lt 100; $i++) {
            $archiveIndex = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'gui-test.zip').ToInt64()
            if ($archiveIndex -ge 0) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($archiveIndex -lt 0) { throw 'Created archive was not listed.' }
        [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$result)
        [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$archiveIndex, 2, 3000, [ref]$result)
        $extraction = Join-Path $configRoot 'extraction'
        New-Item -ItemType Directory -Path $extraction | Out-Null
        Submit-Destination 162 'Extract to existing folder' $extraction
        $confirm = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $confirm = [ZfmSmoke]::FindWindowW('#32770', 'Extract archive')
            if ($confirm -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($confirm -eq [IntPtr]::Zero -or -not [ZfmSmoke]::PostMessageW($confirm, 0x111, [UIntPtr]1, [IntPtr]::Zero)) { throw 'Extract confirmation failed.' }
        $extracted = Join-Path $extraction 'example.txt'
        for ($i = 0; $i -lt 200 -and -not (Test-Path -LiteralPath $extracted); $i++) { Start-Sleep -Milliseconds 100 }
        if (-not (Test-Path -LiteralPath $extracted) -or [IO.File]::ReadAllText($extracted) -ne [IO.File]::ReadAllText((Join-Path $work 'example.txt'))) { throw 'Archive extraction failed.' }
    } else { Write-Host 'Archive GUI smoke skipped: engine not present beside executable.' }

    # A multi-item Trash request with overlapping recursive hits must be
    # rejected before touching the Windows shell or the user's Recycle Bin.
    $overlap = Join-Path $work 'trash-overlap'
    New-Item -ItemType Directory -Path $overlap | Out-Null
    $overlapChild = Join-Path $overlap 'trash-overlap-child.txt'
    [IO.File]::WriteAllText($overlapChild, 'must remain')
    $filter = [ZfmSmoke]::GetDlgItem($window, 112)
    [void][ZfmSmoke]::SendMessageW($filter, 0x000c, [UIntPtr]::Zero, 'trash-overlap')
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]168, [IntPtr]::Zero, 2, 3000, [ref]$result)
    $parentIndex = -1; $childIndex = -1
    for ($i = 0; $i -lt 100; $i++) {
        $parentIndex = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'trash-overlap').ToInt64()
        $childIndex = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'trash-overlap\trash-overlap-child.txt').ToInt64()
        if ($parentIndex -ge 0 -and $childIndex -ge 0) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($parentIndex -lt 0 -or $childIndex -lt 0) { throw 'Trash overlap search did not find both items.' }
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$result)
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$parentIndex, 2, 3000, [ref]$result)
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$childIndex, 2, 3000, [ref]$result)
    [void][ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]135, [IntPtr]::Zero, 2, 3000, [ref]$result)
    $rejected = $false
    for ($i = 0; $i -lt 100; $i++) {
        $status = New-Object System.Text.StringBuilder 1024
        [void][ZfmSmoke]::GetWindowTextW([ZfmSmoke]::GetDlgItem($window, 108), $status, $status.Capacity)
        if ($status.ToString() -match 'must not contain one another') { $rejected = $true; break }
        Start-Sleep -Milliseconds 100
    }
    if (-not $rejected -or [IO.File]::ReadAllText($overlapChild) -ne 'must remain') { throw 'Overlapping trash selection was not safely rejected.' }

    # Search nested names and navigate to the result's containing directory.
    $searchFolder = Join-Path $work 'search-fixture'
    New-Item -ItemType Directory -Path $searchFolder -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $searchFolder 'needle-search.txt'), 'nested search fixture')
    $filter = [ZfmSmoke]::GetDlgItem($window, 112)
    if ([ZfmSmoke]::SendMessageW($filter, 0x000c, [UIntPtr]::Zero, 'needle-search.txt') -eq [IntPtr]::Zero) { throw 'Could not set search query.' }
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]168, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not start recursive search.' }
    $searchIndex = -1
    for ($i = 0; $i -lt 100; $i++) {
        $searchIndex = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, 'search-fixture\needle-search.txt').ToInt64()
        if ($searchIndex -ge 0) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($searchIndex -lt 0) { throw 'Recursive search did not list the nested path.' }
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$result)
    [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$searchIndex, 2, 3000, [ref]$result)
    if ([ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]169, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not open containing folder.' }
    $address = [ZfmSmoke]::GetDlgItem($window, 106)
    $opened = $false
    for ($i = 0; $i -lt 100; $i++) {
        $value = New-Object System.Text.StringBuilder 2048
        [void][ZfmSmoke]::GetControlText($address, 0x000d, [UIntPtr]$value.Capacity, $value, 2, 3000, [ref]$result)
        if ($value.ToString() -eq $searchFolder) { $opened = $true; break }
        Start-Sleep -Milliseconds 100
    }
    if (-not $opened) { throw 'Containing folder did not open.' }
    [void][ZfmSmoke]::SendMessageW($address, 0x000c, [UIntPtr]::Zero, $work)
    [void][ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]105, [IntPtr]::Zero, 2, 3000, [ref]$result)

    # Multi-item cut/paste, including a partial move and safe clipboard cleanup.
    $cutSource = Join-Path $configRoot 'cut-source'
    $cutTarget = Join-Path $configRoot 'cut-target'
    New-Item -ItemType Directory -Path (Join-Path $cutSource 'folder-a') -Force | Out-Null
    New-Item -ItemType Directory -Path $cutTarget | Out-Null
    [IO.File]::WriteAllText((Join-Path $cutSource 'folder-a\nested.txt'), 'nested cut')
    [IO.File]::WriteAllText((Join-Path $cutSource 'z-conflict.txt'), 'original cut')
    [IO.File]::WriteAllText((Join-Path $cutTarget 'z-conflict.txt'), 'existing')
    function Navigate-Cut([string]$path, [string]$expected) {
        # A pending refresh may reset the address between WM_SETTEXT and GO.
        # Retry until the completed listing explicitly reports the target path.
        for ($attempt = 0; $attempt -lt 4; $attempt++) {
            [void][ZfmSmoke]::SendMessageW($address, 0x000c, [UIntPtr]::Zero, $path)
            $r = [UIntPtr]::Zero
            [void][ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]105, [IntPtr]::Zero, 2, 3000, [ref]$r)
            for ($i = 0; $i -lt 100; $i++) {
                $status = New-Object System.Text.StringBuilder 1024
                [void][ZfmSmoke]::GetWindowTextW([ZfmSmoke]::GetDlgItem($window, 108), $status, $status.Capacity)
                if ($status.ToString().EndsWith("  |  $path") -and [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, $expected).ToInt64() -ge 0) { return }
                if ($status.ToString() -notmatch '^Loading ') { break }
                Start-Sleep -Milliseconds 100
            }
        }
        throw "Cut fixture $expected was not listed in $path (status: $($status.ToString()))."
    }
    function Select-Cut([string[]]$names) {
        $r = [UIntPtr]::Zero
        [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]::Zero, [IntPtr](-1), 2, 3000, [ref]$r)
        foreach ($name in $names) {
            $index = [ZfmSmoke]::SendMessageW($list, 0x1a2, [UIntPtr]::Zero, $name).ToInt64()
            if ($index -lt 0) { throw "Could not select $name for cut." }
            [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x185, [UIntPtr]1, [IntPtr]$index, 2, 3000, [ref]$r)
        }
    }
    function Invoke-CutCommand([int]$command) {
        $r = [UIntPtr]::Zero
        if ([ZfmSmoke]::SendMessageTimeoutW($window, 0x111, [UIntPtr]$command, [IntPtr]::Zero, 2, 3000, [ref]$r) -eq [IntPtr]::Zero) { throw "Cut/paste command $command failed." }
    }
    function Wait-CutStatus([string]$pattern) {
        for ($i = 0; $i -lt 100; $i++) {
            $text = New-Object System.Text.StringBuilder 1024
            [void][ZfmSmoke]::GetWindowTextW([ZfmSmoke]::GetDlgItem($window, 108), $text, $text.Capacity)
            if ($text.ToString() -match $pattern) { return }
            Start-Sleep -Milliseconds 100
        }
        $details = "status=$($text.ToString()); source=$((Get-ChildItem -LiteralPath $cutSource -Name) -join ','); destination=$((Get-ChildItem -LiteralPath $cutTarget -Name) -join ','); clipboard=$([ZfmSmoke]::IsClipboardFormatAvailable(15))"
        throw "Cut/paste did not report $pattern; $details"
    }
    Navigate-Cut $cutSource 'folder-a'
    Select-Cut @('folder-a', 'z-conflict.txt')
    Invoke-CutCommand 131
    if (-not [ZfmSmoke]::IsClipboardFormatAvailable(15)) { throw 'Cut did not publish a file clipboard.' }
    # Pasting into the source folder fails without consuming the cut clipboard.
    Invoke-CutCommand 132
    Wait-CutStatus 'Moved 0 of 2'
    if (-not [ZfmSmoke]::IsClipboardFormatAvailable(15)) { throw 'A zero-progress failure cleared the cut clipboard.' }
    Navigate-Cut $cutTarget 'z-conflict.txt'
    Invoke-CutCommand 132
    Wait-CutStatus 'Moved 1 of 2'
    if ([IO.File]::ReadAllText((Join-Path $cutTarget 'folder-a\nested.txt')) -ne 'nested cut' -or
        [IO.File]::ReadAllText((Join-Path $cutTarget 'z-conflict.txt')) -ne 'existing' -or
        [IO.File]::ReadAllText((Join-Path $cutSource 'z-conflict.txt')) -ne 'original cut') { throw 'Partial cut/paste did not preserve the conflict.' }
    if ([ZfmSmoke]::IsClipboardFormatAvailable(15)) { throw 'Consumed cut clipboard was not cleared after partial move.' }
    Remove-Item -LiteralPath (Join-Path $cutTarget 'z-conflict.txt')
    Navigate-Cut $cutSource 'z-conflict.txt'
    Select-Cut @('z-conflict.txt')
    Invoke-CutCommand 131
    Navigate-Cut $cutTarget 'folder-a'
    Invoke-CutCommand 132
    Wait-CutStatus 'Moved 1 items'
    if ([IO.File]::ReadAllText((Join-Path $cutTarget 'z-conflict.txt')) -ne 'original cut' -or [ZfmSmoke]::IsClipboardFormatAvailable(15)) { throw 'Retry cut/paste failed.' }
    [IO.File]::WriteAllText((Join-Path $cutSource 'a.txt'), 'a')
    [IO.File]::WriteAllText((Join-Path $cutSource 'b.txt'), 'b')
    Navigate-Cut $cutSource 'a.txt'
    Select-Cut @('a.txt', 'b.txt')
    Invoke-CutCommand 131
    Navigate-Cut $cutTarget 'z-conflict.txt'
    Invoke-CutCommand 132
    Wait-CutStatus 'Moved 2 items'
    if ([IO.File]::ReadAllText((Join-Path $cutTarget 'a.txt')) -ne 'a' -or
        [IO.File]::ReadAllText((Join-Path $cutTarget 'b.txt')) -ne 'b' -or
        [ZfmSmoke]::IsClipboardFormatAvailable(15)) { throw 'Multi-item cut/paste failed.' }
    # Case-sensitive filtering uses the same core for current and recursive views.
    $caseSource = Join-Path $configRoot 'case-source'
    New-Item -ItemType Directory -Path (Join-Path $caseSource 'sub') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $caseSource 'MiXeD.txt'), 'case')
    [IO.File]::WriteAllText((Join-Path $caseSource 'sub\MiXeD.txt'), 'nested')
    Navigate-Cut $caseSource 'MiXeD.txt'
    [void][ZfmSmoke]::SendMessageW($filter, 0x000c, [UIntPtr]::Zero, 'mixed.txt')
    function Wait-CaseStatus([string]$pattern) {
        for ($i = 0; $i -lt 100; $i++) {
            $state = New-Object System.Text.StringBuilder 2048
            [void][ZfmSmoke]::GetWindowTextW([ZfmSmoke]::GetDlgItem($window, 108), $state, $state.Capacity)
            if ($state.ToString() -match $pattern) { return }
            Start-Sleep -Milliseconds 100
        }
        throw "Case-sensitive search did not report $pattern; status=$state"
    }
    Invoke-CutCommand 113
    Wait-CaseStatus '^1 items  \|'
    Invoke-CutCommand 173
    Wait-CaseStatus '^0 items  \|'
    Invoke-CutCommand 168
    Wait-CaseStatus '^Recursive search: 0 results'
    Invoke-CutCommand 173
    Wait-CaseStatus '^Recursive search: 2 results'
    Invoke-CutCommand 114
    # Calculate apparent size across a folder (including hidden data) and a file.
    $sizeSource = Join-Path $configRoot 'size-source'
    New-Item -ItemType Directory -Path (Join-Path $sizeSource 'folder') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $sizeSource 'folder\nested.txt'), 'abc')
    [IO.File]::WriteAllText((Join-Path $sizeSource 'second.txt'), 'xyz')
    Navigate-Cut $sizeSource 'folder'
    Select-Cut @('folder', 'second.txt')
    [void][ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]171, [IntPtr]::Zero)
    $sizeDialog = [IntPtr]::Zero; $sizeText = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $sizeDialog = [ZfmSmoke]::FindWindowW('#32770', 'Size results')
        if ($sizeDialog -ne [IntPtr]::Zero) { $sizeText = [ZfmSmoke]::GetDlgItem($sizeDialog, 1002) }
        if ($sizeText -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($sizeText -eq [IntPtr]::Zero) { throw 'Size results did not appear.' }
    $value = New-Object System.Text.StringBuilder 4096
    $r = [UIntPtr]::Zero
    [void][ZfmSmoke]::GetControlText($sizeText, 0x000d, [UIntPtr]$value.Capacity, $value, 2, 3000, [ref]$r)
    if ($value.ToString() -notmatch 'Scan finished' -or $value.ToString() -notmatch 'Detailed Report:' -or $value.ToString() -notmatch 'size: 6 bytes' -or
        $value.ToString() -notmatch 'Files: 2' -or $value.ToString() -notmatch 'Folders: 1') { throw "Incorrect size results: $value" }
    [void][ZfmSmoke]::PostMessageW($sizeDialog, 0x111, [UIntPtr]1, [IntPtr]::Zero)
    for ($i = 0; $i -lt 100 -and [ZfmSmoke]::FindWindowW('#32770', 'Size results') -ne [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 100 }
    # Numbered batch rename shows every mapping before any change.
    $renameSource = Join-Path $configRoot 'rename-source'
    New-Item -ItemType Directory -Path $renameSource | Out-Null
    [IO.File]::WriteAllText((Join-Path $renameSource 'alpha.txt'), 'alpha')
    [IO.File]::WriteAllText((Join-Path $renameSource 'beta.csv'), 'beta')
    Navigate-Cut $renameSource 'alpha.txt'
    function Open-BatchRename {
        Select-Cut @('alpha.txt', 'beta.csv')
        if (-not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]138, [IntPtr]::Zero)) { throw 'Could not request batch rename.' }
        $dialog = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $dialog = [ZfmSmoke]::FindWindowW('#32770', 'Batch rename — base name')
            if ($dialog -ne [IntPtr]::Zero -and [ZfmSmoke]::GetDlgItem($dialog, 1001) -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($dialog -eq [IntPtr]::Zero) { throw 'Batch rename base dialog did not appear.' }
        $edit = [ZfmSmoke]::GetDlgItem($dialog, 1001)
        [void][ZfmSmoke]::SendMessageW($edit, 0x000c, [UIntPtr]::Zero, 'Holiday')
        [void][ZfmSmoke]::PostMessageW($dialog, 0x111, [UIntPtr]1, [IntPtr]::Zero)
        $preview = [IntPtr]::Zero; $previewText = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $preview = [ZfmSmoke]::FindWindowW('#32770', 'Preview batch rename')
            if ($preview -ne [IntPtr]::Zero) { $previewText = [ZfmSmoke]::GetDlgItem($preview, 1002) }
            if ($previewText -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($previewText -eq [IntPtr]::Zero) {
            $status = New-Object System.Text.StringBuilder 2048
            [void][ZfmSmoke]::GetWindowTextW([ZfmSmoke]::GetDlgItem($window, 108), $status, $status.Capacity)
            $selectedCount = [UIntPtr]::Zero
            [void][ZfmSmoke]::SendMessageTimeoutW($list, 0x190, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$selectedCount)
            throw "Batch rename preview did not appear: preview=$preview status=$($status.ToString()) selection=$($selectedCount.ToUInt64())."
        }
        $value = New-Object System.Text.StringBuilder 8192
        $r = [UIntPtr]::Zero
        [void][ZfmSmoke]::GetControlText($previewText, 0x000d, [UIntPtr]$value.Capacity, $value, 2, 3000, [ref]$r)
        if ($value.ToString() -notmatch 'alpha.txt.*Holiday \(1\).txt' -or $value.ToString() -notmatch 'beta.csv.*Holiday \(2\).csv') { throw "Batch preview omitted a mapping: $($value.ToString())" }
        return $preview
    }
    $preview = Open-BatchRename
    [void][ZfmSmoke]::PostMessageW($preview, 0x111, [UIntPtr]2, [IntPtr]::Zero)
    for ($i = 0; $i -lt 100 -and [ZfmSmoke]::FindWindowW('#32770', 'Preview batch rename') -ne [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path -LiteralPath (Join-Path $renameSource 'alpha.txt')) -or (Test-Path -LiteralPath (Join-Path $renameSource 'Holiday (1).txt'))) { throw 'Cancelling preview changed files.' }
    $preview = Open-BatchRename
    [void][ZfmSmoke]::PostMessageW($preview, 0x111, [UIntPtr]1, [IntPtr]::Zero)
    Wait-CutStatus 'Renamed 2 files'
    if ([IO.File]::ReadAllText((Join-Path $renameSource 'Holiday (1).txt')) -ne 'alpha' -or
        [IO.File]::ReadAllText((Join-Path $renameSource 'Holiday (2).csv')) -ne 'beta' -or
        (Test-Path -LiteralPath (Join-Path $renameSource 'alpha.txt'))) { throw 'Batch rename failed.' }
    # Permanent deletion shows the exact path and defaults to cancellation.
    $deleteFixture = Join-Path $work 'delete-fixture'
    New-Item -ItemType Directory -Path $deleteFixture | Out-Null
    $deleteFile = Join-Path $deleteFixture 'delete-me.txt'
    [IO.File]::WriteAllText($deleteFile, 'delete test')
    Navigate-Cut $deleteFixture 'delete-me.txt'
    function Open-DeleteConfirmation {
        Select-Cut @('delete-me.txt')
        if (-not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]119, [IntPtr]::Zero)) { throw 'Could not request permanent deletion.' }
        $dialog = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $dialog = [ZfmSmoke]::FindWindowW('#32770', 'Confirm permanent deletion')
            if ($dialog -ne [IntPtr]::Zero -and [ZfmSmoke]::GetDlgItem($dialog, 1006) -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($dialog -eq [IntPtr]::Zero) { throw 'Deletion confirmation did not appear.' }
        return $dialog
    }
    $confirmDelete = Open-DeleteConfirmation
    $pathText = New-Object System.Text.StringBuilder 4096
    $result = [UIntPtr]::Zero
    [void][ZfmSmoke]::GetControlText([ZfmSmoke]::GetDlgItem($confirmDelete, 1006), 0x000d, [UIntPtr]$pathText.Capacity, $pathText, 2, 3000, [ref]$result)
    if ($pathText.ToString() -ne $deleteFile) { throw "Deletion confirmation showed the wrong path: $pathText" }
    if ([ZfmSmoke]::SendMessageTimeoutW($confirmDelete, 0x0400, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero -or ($result.ToUInt64() -band 0xffff) -ne 2) { throw 'Deletion does not default to Cancel.' }
    [void][ZfmSmoke]::PostMessageW($confirmDelete, 0x111, [UIntPtr]2, [IntPtr]::Zero)
    for ($i = 0; $i -lt 100 -and [ZfmSmoke]::FindWindowW('#32770', 'Confirm permanent deletion') -ne [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path -LiteralPath $deleteFile)) { throw 'Cancelling deletion removed the file.' }
    $confirmDelete = Open-DeleteConfirmation
    [void][ZfmSmoke]::PostMessageW($confirmDelete, 0x111, [UIntPtr]1, [IntPtr]::Zero)
    for ($i = 0; $i -lt 100 -and (Test-Path -LiteralPath $deleteFile); $i++) { Start-Sleep -Milliseconds 100 }
    if (Test-Path -LiteralPath $deleteFile) { throw 'Confirmed deletion did not remove the file.' }
    # A populated list still responds to native scrolling with the custom skin.
    $scrollFixture = Join-Path $work 'scroll-fixture'
    New-Item -ItemType Directory -Path $scrollFixture | Out-Null
    for ($i = 0; $i -lt 80; $i++) { [IO.File]::WriteAllText((Join-Path $scrollFixture ("item-{0:D3}.txt" -f $i)), 'scroll') }
    Navigate-Cut $scrollFixture 'item-079.txt'
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x0115, [UIntPtr]7, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'List scroll failed.' }
    if ([ZfmSmoke]::SendMessageTimeoutW($list, 0x018e, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero -or $result.ToUInt64() -eq 0) { throw 'List did not scroll to the bottom.' }
    # Review-only insights: persist and compare a baseline, explain cleanup,
    # and preview a conflicting rename without changing the selected file.
    $insightFolder = Join-Path $work 'insight-fixture'
    New-Item -ItemType Directory -Path $insightFolder | Out-Null
    [IO.File]::WriteAllText((Join-Path $insightFolder 'before.txt'), 'before')
    Navigate-Cut $insightFolder 'before.txt'
    function Read-InsightReport([string]$title, [string]$pattern) {
        $dialog = [IntPtr]::Zero; $content = [IntPtr]::Zero
        for ($i = 0; $i -lt 100; $i++) {
            $dialog = [ZfmSmoke]::FindWindowW('#32770', $title)
            if ($dialog -ne [IntPtr]::Zero) { $content = [ZfmSmoke]::GetDlgItem($dialog, 1002) }
            if ($content -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($content -eq [IntPtr]::Zero) { throw "$title report did not appear." }
        $buffer = New-Object System.Text.StringBuilder 32768; $r = [UIntPtr]::Zero
        [void][ZfmSmoke]::GetControlText($content, 0x000d, [UIntPtr]$buffer.Capacity, $buffer, 2, 3000, [ref]$r)
        if ($buffer.ToString() -notmatch $pattern) { throw "Unexpected $title report: $buffer" }
        [void][ZfmSmoke]::PostMessageW($dialog, 0x111, [UIntPtr]1, [IntPtr]::Zero)
        for ($i = 0; $i -lt 100 -and [ZfmSmoke]::FindWindowW('#32770', $title) -ne [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 100 }
    }
    [void][ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]174, [IntPtr]::Zero)
    $confirmSnapshot = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $confirmSnapshot = [ZfmSmoke]::FindWindowW('#32770', 'Save folder snapshot?')
        if ($confirmSnapshot -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($confirmSnapshot -eq [IntPtr]::Zero) { throw 'Snapshot confirmation did not appear.' }
    [void][ZfmSmoke]::PostMessageW($confirmSnapshot, 0x111, [UIntPtr]1, [IntPtr]::Zero)
    Read-InsightReport 'Folder snapshot' 'Saved a baseline'
    Remove-Item -LiteralPath (Join-Path $insightFolder 'before.txt')
    [IO.File]::WriteAllText((Join-Path $insightFolder 'after.tmp'), '')
    Navigate-Cut $insightFolder 'after.tmp'
    [void][ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]175, [IntPtr]::Zero)
    Read-InsightReport 'Folder changes' 'Removed:.*before.txt[\s\S]*Added:.*after.tmp'
    [void][ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]176, [IntPtr]::Zero)
    Read-InsightReport 'Cleanup review' 'empty file.*temporary/backup-like name'
    if (-not (Test-Path -LiteralPath (Join-Path $insightFolder 'after.tmp'))) { throw 'Cleanup review changed files.' }
    Select-Cut @('after.tmp')
    Submit-Destination 179 'Preview rename impact' 'after.tmp'
    Read-InsightReport 'Operation impact preview' 'destination already exists'
    if (-not (Test-Path -LiteralPath (Join-Path $insightFolder 'after.tmp'))) { throw 'Impact preview changed files.' }
    Navigate-Cut $work 'example.txt'

    # Save a changed value after the other list tests, then verify it survives restart.
    if (-not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]143, [IntPtr]::Zero)) { throw 'Could not request settings for saving.' }
    $settings = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $settings = [ZfmSmoke]::FindWindowW('#32770', 'ZFM Settings')
        if ($settings -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($settings -eq [IntPtr]::Zero) { throw 'Settings did not open for saving.' }
    $hiddenSetting = [ZfmSmoke]::GetDlgItem($settings, 1001)
    $compactSetting = [ZfmSmoke]::GetDlgItem($settings, 1005)
    $refreshSetting = [ZfmSmoke]::GetDlgItem($settings, 1004)
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($refreshSetting, 0x00f1, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not disable automatic refresh.' }
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($compactSetting, 0x00f1, [UIntPtr]1, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not change compact view.' }
    $result = [UIntPtr]::Zero
    if ([ZfmSmoke]::SendMessageTimeoutW($hiddenSetting, 0x00f1, [UIntPtr]1, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Could not change the setting.' }
    if (-not [ZfmSmoke]::PostMessageW($settings, 0x111, [UIntPtr]1, [IntPtr]::Zero)) { throw 'Could not apply settings.' }
    for ($i = 0; $i -lt 100 -and -not (Test-Path -LiteralPath $configFile); $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path -LiteralPath $configFile) -or ([IO.File]::ReadAllText($configFile) -notmatch 'show_hidden=true')) { throw 'Settings were not saved.' }
    # Relaunch with the same configuration directory and check the saved setting.
    Stop-Process -Id $process.Id -Force
    $process.WaitForExit()
    $process = Start-Process -FilePath (Resolve-Path -LiteralPath $Executable).Path -ArgumentList ('"' + $work + '"') -PassThru
    $window = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $process.Refresh()
        if ($process.HasExited) { throw 'ZFM exited while restoring settings.' }
        $window = $process.MainWindowHandle
        if ($window -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($window -eq [IntPtr]::Zero -or -not [ZfmSmoke]::PostMessageW($window, 0x111, [UIntPtr]143, [IntPtr]::Zero)) { throw 'Cannot reopen settings after restart.' }
    $settings = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        $settings = [ZfmSmoke]::FindWindowW('#32770', 'ZFM Settings')
        if ($settings -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($settings -eq [IntPtr]::Zero) { throw 'Settings did not reopen after restart.' }
    $result = [UIntPtr]::Zero
    $hiddenSetting = [ZfmSmoke]::GetDlgItem($settings, 1001)
    if ([ZfmSmoke]::SendMessageTimeoutW($hiddenSetting, 0x00f0, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero -or $result.ToUInt64() -ne 1) { throw 'Hidden-file setting was not restored.' }
    $result = [UIntPtr]::Zero
    $compactSetting = [ZfmSmoke]::GetDlgItem($settings, 1005)
    if ([ZfmSmoke]::SendMessageTimeoutW($compactSetting, 0x00f0, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero -or $result.ToUInt64() -ne 1) { throw 'Compact-view setting was not restored.' }
    $result = [UIntPtr]::Zero
    $refreshSetting = [ZfmSmoke]::GetDlgItem($settings, 1004)
    if ([ZfmSmoke]::SendMessageTimeoutW($refreshSetting, 0x00f0, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$result) -eq [IntPtr]::Zero -or $result.ToUInt64() -ne 0) { throw 'Automatic-refresh setting was not restored.' }
    [void][ZfmSmoke]::PostMessageW($settings, 0x10, [UIntPtr]::Zero, [IntPtr]::Zero)
    Write-Host "Win32 window and basic controls detected: $title"
} catch {
    Write-Host "::error title=GUI smoke test::$($_.Exception.Message)"
    throw
} finally {
    if ($null -ne $process) {
        $process.Refresh()
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force; $process.WaitForExit() }
    }
    $env:APPDATA = $originalAppData
    Remove-Item -LiteralPath $work, $configRoot -Recurse -Force -ErrorAction SilentlyContinue
}

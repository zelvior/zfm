#Requires -Version 5.1
<#
.SYNOPSIS
  Install/update Rust and build ZFM for Windows, optionally also Linux via WSL.
.EXAMPLE
  .\scripts\build-windows.cmd
.EXAMPLE
  .\scripts\build-windows.cmd -IncludeLinux
#>
[CmdletBinding()]
param(
    [switch]$NativeOnly,
    [switch]$IncludeLinux,
    [switch]$SkipTests,
    [string]$Toolchain = 'stable'
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($Toolchain)) { Write-Error 'Specify a non-empty Rust toolchain.'; exit 2 }
$Root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $Root
if ($env:OS -ne 'Windows_NT') { Write-Error 'This script requires Windows.'; exit 1 }
$Architecture = $env:PROCESSOR_ARCHITECTURE
if ($env:PROCESSOR_ARCHITEW6432) { $Architecture = $env:PROCESSOR_ARCHITEW6432 }
switch ($Architecture.ToUpperInvariant()) {
    'AMD64' { $HostTarget = 'x86_64-pc-windows-msvc'; $InstallerUrl = 'https://win.rustup.rs/x86_64' }
    'X86' { $HostTarget = 'i686-pc-windows-msvc'; $InstallerUrl = 'https://win.rustup.rs/i686' }
    default { Write-Error "Unsupported Windows host architecture: $Architecture. This script supports x86 and x86_64 hosts."; exit 1 }
}

# Respect rustup's user configuration and do not edit Windows PATH permanently.
$CargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$CargoBin = Join-Path $CargoHome 'bin'
$Rustup = Get-Command rustup.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if ($Rustup) {
    $RustupPath = $Rustup.Source
} elseif (Test-Path -LiteralPath (Join-Path $CargoBin 'rustup.exe') -PathType Leaf) {
    $RustupPath = Join-Path $CargoBin 'rustup.exe'
} else {
    $InstallerDir = Join-Path ([IO.Path]::GetTempPath()) ('zfm-rustup-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $InstallerDir | Out-Null
    $Installer = Join-Path $InstallerDir 'rustup-init.exe'
    try {
        Write-Host "Downloading the official rustup installer from $InstallerUrl"
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri $InstallerUrl -OutFile $Installer -UseBasicParsing
        $Process = Start-Process -FilePath $Installer -ArgumentList @('-y', '--no-modify-path', '--profile', 'minimal', '--default-toolchain', 'none', '--default-host', $HostTarget) -Wait -PassThru
        if ($Process.ExitCode -ne 0) { throw "rustup installer exited with code $($Process.ExitCode)." }
    } finally {
        Remove-Item -LiteralPath $InstallerDir -Recurse -Force -ErrorAction SilentlyContinue
    }
    $RustupPath = Join-Path $CargoBin 'rustup.exe'
    if (-not (Test-Path -LiteralPath $RustupPath -PathType Leaf)) { throw "Cannot find rustup at $RustupPath" }
}
$env:PATH = "$CargoBin;$env:PATH"
Write-Host "Installing/updating Rust toolchain: $Toolchain"
& $RustupPath toolchain install $Toolchain --profile minimal
if ($LASTEXITCODE -ne 0) { throw 'Rust toolchain installation failed. Check network and OS compatibility, or choose an older compatible -Toolchain version.' }
& $RustupPath run $Toolchain rustc --version
if ($LASTEXITCODE -ne 0) { throw 'Could not run rustc.' }
& $RustupPath run $Toolchain cargo --version
if ($LASTEXITCODE -ne 0) { throw 'Could not run Cargo.' }

Write-Host 'Windows builds need Visual Studio C++ Build Tools with the Desktop development with C++ workload and Windows SDK (x86 and x64).'
Write-Host 'If a link fails, install those tools and retry. They are not installed or modified by this script.'
# Keep target output under this checkout, independent of the user Cargo config.
$env:CARGO_TARGET_DIR = Join-Path $Root 'target\zfm-build-windows'
if (-not $SkipTests) {
    & $RustupPath target add --toolchain $Toolchain $HostTarget
    if ($LASTEXITCODE -ne 0) { throw "Could not install the native Rust target $HostTarget." }
    Write-Host 'Running native automated tests...'
    & $RustupPath run $Toolchain cargo test --locked --target $HostTarget
    if ($LASTEXITCODE -ne 0) { throw 'Native tests failed; no executables published. Check Visual Studio C++ Build Tools and SDK. Use -SkipTests only if you understand the risk.' }
}
$ArtifactRoot = Join-Path $Root 'artifacts'
New-Item -ItemType Directory -Path $ArtifactRoot -Force | Out-Null
$Output = Join-Path $ArtifactRoot ('build-windows-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Output | Out-Null
$Targets = if ($NativeOnly) { @($HostTarget) } else { @('x86_64-pc-windows-msvc', 'i686-pc-windows-msvc') }
$Succeeded = @()
$Failed = @()
foreach ($Target in $Targets) {
    Write-Host "`n=== $Target ==="
    & $RustupPath target add --toolchain $Toolchain $Target
    if ($LASTEXITCODE -ne 0) { $Failed += "$Target (Rust target installation failed)"; continue }
    & $RustupPath run $Toolchain cargo build --release --locked --target $Target
    if ($LASTEXITCODE -ne 0) { $Failed += "$Target (build or linker failed)"; continue }
    $Binary = Join-Path $env:CARGO_TARGET_DIR "$Target\release\zfm.exe"
    $Destination = Join-Path $Output $Target
    try {
        if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) { throw "Executable not found: $Binary" }
        New-Item -ItemType Directory -Path $Destination | Out-Null
        & (Join-Path $PSScriptRoot 'build-archive-engine.ps1') -Target $Target -Output (Join-Path $Destination 'zfm-archive-engine.exe')
        & python (Join-Path $PSScriptRoot 'prepare-pdfium.py') $Target $Destination
        if ($LASTEXITCODE -ne 0) { throw 'Could not prepare bundled PDF renderer; install Python 3 and retry.' }
        Copy-Item -LiteralPath $Binary -Destination (Join-Path $Destination 'zfm.exe')
        Copy-Item -LiteralPath (Join-Path $Root 'vendor/7zip/7zip-d3296e39.tar.gz') -Destination (Join-Path $Destination '7zip-source.tar.gz')
        Copy-Item -LiteralPath (Join-Path $Root 'LICENSES/7zip/License.txt') -Destination (Join-Path $Destination '7zip-notices.txt')
        Copy-Item -LiteralPath (Join-Path $Root 'LICENSES/7zip/copying.txt') -Destination (Join-Path $Destination '7zip-lgpl.txt')
        Copy-Item -LiteralPath (Join-Path $Root 'LICENSES/7zip/unRarLicense.txt') -Destination (Join-Path $Destination '7zip-unrar.txt')
        $Succeeded += (Join-Path $Destination 'zfm.exe')
        Write-Host "Created $(Join-Path $Destination 'zfm.exe')"
    } catch {
        Write-Warning "Could not publish ${Target}: $_"
        $Failed += "$Target (publication failed)"
    }
}
if ($IncludeLinux) {
    Write-Host "`n=== Linux builds through WSL ==="
    $Wsl = Get-Command wsl.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $Wsl) {
        $Failed += 'Linux (WSL is not installed or not on PATH)'
    } else {
        Write-Host 'WSL will run the Linux build script and may install/update rustup in the WSL user account.'
        Write-Host 'Install Linux native and multilib C compilers in WSL first; see README.'
        # Pass paths as separate arguments; never interpolate them into shell code.
        $LinuxRoot = & $Wsl.Source --exec wslpath -a $Root
        if ($LASTEXITCODE -ne 0 -or -not $LinuxRoot) {
            $Failed += 'Linux (WSL could not translate the checkout path)'
        } else {
            $LinuxArgs = @('--linux-only')
            if ($SkipTests) { $LinuxArgs += '--skip-tests' }
            & $Wsl.Source --exec bash "$LinuxRoot/scripts/build-linux.sh" @LinuxArgs
            if ($LASTEXITCODE -ne 0) { $Failed += 'Linux (WSL build failed; see its output above)' }
            else { Write-Host 'Linux executables are in the separate artifacts/build-* directory printed above.' }
        }
    }
}
Write-Host "`nWindows build output: $Output"
if ($Succeeded.Count -gt 0) { Write-Host 'Built:'; $Succeeded | ForEach-Object { Write-Host "  $_" } }
if ($Failed.Count -gt 0) {
    Write-Host 'Failed or unavailable:'
    $Failed | ForEach-Object { Write-Host "  $_" }
    Write-Host 'Successful executables were retained. Install missing prerequisites and rerun.'
    exit 1
}
Write-Host 'All requested builds succeeded. Test the executables on their target systems before relying on them.'

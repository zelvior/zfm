#Requires -Version 5.1
# Build the pinned upstream standalone engine from source without downloading it.
param([Parameter(Mandatory=$true)][string]$Target,
      [Parameter(Mandatory=$true)][string]$Output)
$ErrorActionPreference = 'Stop'
switch ($Target) {
    'x86_64-pc-windows-msvc' { $arch = 'x64' }
    'i686-pc-windows-msvc' { $arch = 'x86' }
    default { throw "Unsupported Windows target: $Target" }
}
$root = Split-Path -Parent $PSScriptRoot
$source = Join-Path $root 'vendor\7zip\7zip-d3296e39.tar.gz'
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $source).Hash.ToLowerInvariant()
if ($hash -ne 'adef737a73a2668d26c0733e496e1600662af2a8779e084e6475a0162e012576') { throw 'Wrong 7-Zip source checksum' }
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Visual Studio C++ Build Tools not found' }
$vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvarsall.bat'
$temp = Join-Path ([IO.Path]::GetTempPath()) ('zfm-engine-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null
try {
    & tar.exe -xzf $source -C $temp --strip-components 1
    if ($LASTEXITCODE -ne 0) { throw 'Could not extract the upstream source archive' }
    $bundle = Join-Path $temp 'CPP\7zip\Bundles\Alone2'
    # Use a batch file instead of nesting cmd /s quotes around multiple quoted
    # paths. nmake inherits the architecture-specific MSVC environment.
    $batch = Join-Path $temp 'build-engine.cmd'
    $commands = @(
        '@echo off',
        ('call "' + $vcvars + '" ' + $arch),
        'if errorlevel 1 exit /b %errorlevel%',
        ('nmake /nologo PLATFORM=' + $arch),
        'exit /b %errorlevel%'
    )
    $encoding = [Text.Encoding]::GetEncoding([Globalization.CultureInfo]::CurrentCulture.TextInfo.OEMCodePage)
    [IO.File]::WriteAllLines($batch, [string[]]$commands, $encoding)
    Push-Location -LiteralPath $bundle
    try { & $env:ComSpec /d /c $batch; $buildExit = $LASTEXITCODE }
    finally { Pop-Location }
    if ($buildExit -ne 0) { throw 'Archive engine build failed' }
    $built = Join-Path $bundle "$arch\7zz.exe"
    if (-not (Test-Path -LiteralPath $built -PathType Leaf)) { throw "Archive engine was not produced: $built" }
    $destination = [IO.Path]::GetFullPath($Output)
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    Copy-Item -LiteralPath $built -Destination $destination -Force
} finally {
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}

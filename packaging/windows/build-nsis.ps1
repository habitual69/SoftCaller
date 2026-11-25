# Builds the per-user Softcaller NSIS installer.
#
#   pwsh packaging/windows/build-nsis.ps1
#   pwsh packaging/windows/build-nsis.ps1 -SkipBuild -Version 0.1.0
#
# It needs NSIS 3 (makensis). Install it with:
#   winget install NSIS.NSIS
#
# The output defaults to dist/Softcaller-<version>-x64-setup.exe.

[CmdletBinding()]
param(
    [string]$Version,
    [string]$BinaryDir = (Join-Path $PSScriptRoot '..\..\target\release'),
    [string]$OutDir = (Join-Path $PSScriptRoot '..\..\dist'),
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'softcaller.nsi'

if (-not $Version) {
    $metadata = cargo metadata --no-deps --format-version 1 --offline | ConvertFrom-Json
    $Version = ($metadata.packages | Where-Object { $_.name -eq 'softcaller-app' }).version
}
if (-not $Version) {
    throw 'Cannot read the Softcaller version from cargo metadata; pass -Version.'
}

$BinaryDir = [IO.Path]::GetFullPath($BinaryDir)
$OutDir = [IO.Path]::GetFullPath($OutDir)
$outFile = Join-Path $OutDir "Softcaller-$Version-x64-setup.exe"
$exe = Join-Path $BinaryDir 'softcaller.exe'

if (-not $SkipBuild) {
    Write-Host "Building Softcaller $Version (release)…"
    cargo build -p softcaller-app --release
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
}
if (-not (Test-Path -LiteralPath $exe)) {
    throw "softcaller.exe not found in $BinaryDir; build the release binary first."
}

$makensis = Get-Command makensis -ErrorAction SilentlyContinue |
    Select-Object -ExpandProperty Source -First 1
if (-not $makensis) {
    $candidates = @(
        "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
        "$env:ProgramFiles\NSIS\makensis.exe"
    )
    $makensis = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not $makensis) {
    throw 'makensis not found. Install NSIS 3 with: winget install NSIS.NSIS'
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Write-Host "Building $outFile…"
& $makensis /V2 "/DVERSION=$Version" "/DBINARY_DIR=$BinaryDir" "/DOUTFILE=$outFile" $script
if ($LASTEXITCODE -ne 0) { throw "makensis failed with exit code $LASTEXITCODE" }
if (-not (Test-Path -LiteralPath $outFile)) { throw "The installer was not created: $outFile" }

Get-Item -LiteralPath $outFile | Select-Object FullName, Length, LastWriteTime

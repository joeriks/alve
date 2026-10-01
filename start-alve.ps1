param(
    [int]$Port = 4785,
    [string]$DataDirectory = 'private-vaults/native',
    [string]$ExecutablePath = ''
)
$ErrorActionPreference = 'Stop'
if (-not $ExecutablePath) {
    $alveCandidates = @((Join-Path $PSScriptRoot 'target/release/alve.exe'), (Join-Path $PSScriptRoot 'target/debug/alve.exe'))
    $ExecutablePath = $alveCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not $ExecutablePath) { throw 'Build Alve with npm ci and npm run build -- --no-bundle, or provide -ExecutablePath to a downloaded native build.' }
$env:ALVE_DATA_DIR = if ([IO.Path]::IsPathRooted($DataDirectory)) { $DataDirectory } else { Join-Path $PSScriptRoot $DataDirectory }
$env:ALVE_API_PORT = [string]$Port
& $ExecutablePath

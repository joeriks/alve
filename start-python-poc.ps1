param(
    [string]$PythonPath = '',
    [int]$Port = 4765,
    [string]$DataDirectory = 'private-vaults/poc'
)

$ErrorActionPreference = 'Stop'
$alveRoot = $PSScriptRoot
if (-not $PythonPath) {
    $alveCandidates = @(
        (Join-Path $alveRoot '.venv/Scripts/python.exe'),
        (Join-Path $env:USERPROFILE '.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe')
    )
    $PythonPath = $alveCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $PythonPath) {
        $alvePythonCommand = Get-Command python -ErrorAction SilentlyContinue
        if ($alvePythonCommand) { $PythonPath = $alvePythonCommand.Source }
    }
}
if (-not $PythonPath) {
    throw 'Install Python 3.12+, create .venv, and install requirements.txt, or provide -PythonPath.'
}
Push-Location -LiteralPath $alveRoot
try {
    & $PythonPath -m app --port $Port --data-dir $DataDirectory
    if ($LASTEXITCODE -ne 0) { throw 'Alve exited with an error. Check Python dependencies and the data-directory lock.' }
} finally {
    Pop-Location
}

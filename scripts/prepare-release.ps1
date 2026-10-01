[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Version,

    [Parameter(Mandatory)]
    [string]$X64Installer,

    [Parameter(Mandatory)]
    [string]$Arm64Installer,

    [Parameter(Mandatory)]
    [string]$OutputDirectory,

    [string]$KeyPath = (Join-Path $PSScriptRoot '..\private-vaults\release-signing\updater.key'),

    [string]$PasswordPath = (Join-Path $PSScriptRoot '..\private-vaults\release-signing\password.txt')
)

$ErrorActionPreference = 'Stop'

function Resolve-ExistingFile([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label does not exist or is not a file: $Path"
    }
    return (Resolve-Path -LiteralPath $Path).Path
}

function Read-Signature([string]$SignaturePath) {
    $signature = (Get-Content -LiteralPath $SignaturePath -Raw).Trim()
    if ([string]::IsNullOrWhiteSpace($signature)) {
        throw "The signer created an empty signature: $SignaturePath"
    }

    try {
        $decoded = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($signature))
    }
    catch {
        throw "The signer created a signature that is not valid base64: $SignaturePath"
    }

    $trustedComment = $decoded -split "`r?`n" | Where-Object { $_ -like 'trusted comment:*' } | Select-Object -First 1
    if ($null -eq $trustedComment) {
        throw "The signature has no trusted comment: $SignaturePath"
    }
    if (($trustedComment -split "`t") -notcontains "version:$Version") {
        throw "The signature is not bound to version ${Version}: $SignaturePath"
    }
    return $signature
}

function Assert-ReleaseManifest([string]$ManifestPath, [object[]]$ReleaseFiles, [string]$ReleaseBaseUrl) {
    $release = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json
    if ($release.version -ne $Version) {
        throw "latest.json version does not match $Version."
    }
    if ($release.notes -isnot [string] -or [string]::IsNullOrWhiteSpace($release.notes)) {
        throw 'latest.json must contain non-empty release notes.'
    }
    try {
        [DateTimeOffset]::Parse([string]$release.pub_date) | Out-Null
    }
    catch {
        throw 'latest.json pub_date is not a valid timestamp.'
    }
    foreach ($file in $ReleaseFiles) {
        $platform = $release.platforms.($file.Platform)
        if ($null -eq $platform) {
            throw "latest.json is missing platform $($file.Platform)."
        }
        if ($platform.url -ne "$ReleaseBaseUrl/$($file.Name)") {
            throw "latest.json has an unexpected URL for $($file.Platform)."
        }
        if ($platform.signature -ne $file.Signature) {
            throw "latest.json has an unexpected signature for $($file.Platform)."
        }
    }
}

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$configPath = Join-Path $repositoryRoot 'src-tauri\tauri.conf.json'
$config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
if ($config.version -ne $Version) {
    throw "Version $Version does not match src-tauri/tauri.conf.json version $($config.version)."
}

$x64Source = Resolve-ExistingFile $X64Installer 'X64 installer'
$arm64Source = Resolve-ExistingFile $Arm64Installer 'Arm64 installer'
$key = Resolve-ExistingFile $KeyPath 'Signing key'
$password = Resolve-ExistingFile $PasswordPath 'Signing password file'
$tauri = Join-Path $repositoryRoot 'node_modules\.bin\tauri.cmd'
if (-not (Test-Path -LiteralPath $tauri -PathType Leaf)) {
    throw "Tauri CLI was not found: $tauri. Run npm install first."
}

$output = [IO.Path]::GetFullPath($OutputDirectory)
if (-not (Test-Path -LiteralPath $output)) {
    New-Item -ItemType Directory -Path $output | Out-Null
}
if (-not (Test-Path -LiteralPath $output -PathType Container)) {
    throw "OutputDirectory is not a directory: $output"
}

$files = @(
    @{ Source = $x64Source; Name = "Alve_${Version}_windows-x86_64-setup.exe"; Platform = 'windows-x86_64' },
    @{ Source = $arm64Source; Name = "Alve_${Version}_windows-aarch64-setup.exe"; Platform = 'windows-aarch64' }
)
foreach ($file in $files) {
    $destination = Join-Path $output $file.Name
    if (Test-Path -LiteralPath $destination) {
        throw "Refusing to overwrite an existing release asset: $destination"
    }
    Copy-Item -LiteralPath $file.Source -Destination $destination
    $file.Path = $destination
}

$previousPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
try {
    # Read only into the process environment because Tauri's signer accepts the password there.
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content -LiteralPath $password -Raw).TrimEnd("`r", "`n")
    if ([string]::IsNullOrEmpty($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD)) {
        throw "Signing password file is empty: $password"
    }

    foreach ($file in $files) {
        & $tauri signer sign --private-key-path $key --app-version $Version $file.Path
        if ($LASTEXITCODE -ne 0) {
            throw "Tauri signer failed for $($file.Path) with exit code $LASTEXITCODE."
        }
        $file.Signature = Read-Signature "$($file.Path).sig"
    }
}
finally {
    if ($null -eq $previousPassword) {
        Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
    }
    else {
        $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $previousPassword
    }
}

$releaseBase = "https://github.com/joeriks/alve/releases/download/v$Version"
$platforms = [ordered]@{}
foreach ($file in $files) {
    $platforms[$file.Platform] = [ordered]@{
        signature = $file.Signature
        url = "$releaseBase/$($file.Name)"
    }
}
$manifest = [ordered]@{
    version = $Version
    notes = "Alve $Version release."
    pub_date = [DateTime]::UtcNow.ToString('o')
    platforms = $platforms
}
$manifestPath = Join-Path $output 'latest.json'
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath -Encoding utf8NoBOM
Assert-ReleaseManifest $manifestPath $files $releaseBase

$checksums = foreach ($file in $files) {
    $hash = (Get-FileHash -LiteralPath $file.Path -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash *$($file.Name)"
}
$checksums | Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS') -Encoding utf8NoBOM

Write-Host "Prepared signed release assets in $output"

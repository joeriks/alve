[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Apk,
    [Parameter(Mandatory)][string]$SignerJar,
    [Parameter(Mandatory)][string]$Metadata,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Output,
    [string]$KeyPath = (Join-Path $PSScriptRoot '../private-vaults/android-signing/release.p12'),
    [string]$PasswordPath = (Join-Path $PSScriptRoot '../private-vaults/android-signing/password.txt')
)
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $Apk).Path
$signer = (Resolve-Path -LiteralPath $SignerJar).Path
$key = (Resolve-Path -LiteralPath $KeyPath).Path
$meta = Get-Content -LiteralPath $Metadata -Raw | ConvertFrom-Json
if (-not $meta.elements -or @($meta.elements | Where-Object {$_.versionName -ne $Version}).Count) {
    throw 'APK metadata does not match the requested release version.'
}
if ($meta.applicationId -ne 'com.alve.local') { throw 'Unexpected APK application ID.' }
if (Test-Path -LiteralPath $Output) { throw 'Refusing to overwrite a signed APK.' }
$destination = [IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Path (Split-Path $destination) -Force | Out-Null
$previous = $env:ALVE_ANDROID_SIGNING_PASSWORD
try {
    $env:ALVE_ANDROID_SIGNING_PASSWORD = (Get-Content -LiteralPath $PasswordPath -Raw).TrimEnd("`r", "`n")
    if ([string]::IsNullOrWhiteSpace($env:ALVE_ANDROID_SIGNING_PASSWORD)) { throw 'Android signing password is empty.' }
    & java -jar $signer sign --ks $key --ks-key-alias alve --ks-pass env:ALVE_ANDROID_SIGNING_PASSWORD --key-pass env:ALVE_ANDROID_SIGNING_PASSWORD --out $destination $source
    if ($LASTEXITCODE -ne 0) { throw 'Android APK signing failed.' }
    & java -jar $signer verify --verbose --print-certs $destination
    if ($LASTEXITCODE -ne 0) { throw 'Android APK signature verification failed.' }
} finally {
    if ($null -eq $previous) { Remove-Item Env:ALVE_ANDROID_SIGNING_PASSWORD -ErrorAction SilentlyContinue }
    else { $env:ALVE_ANDROID_SIGNING_PASSWORD = $previous }
}
$checksum = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
"$checksum *$([IO.Path]::GetFileName($destination))" | Set-Content -LiteralPath "$destination.sha256" -Encoding utf8NoBOM
Write-Host "Signed Android $Version APK: $destination"

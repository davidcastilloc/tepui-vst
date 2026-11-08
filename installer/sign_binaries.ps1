<#
.SYNOPSIS
    Signs TEPUI Windows binaries (VST3, CLAP, Standalone EXE, and SFX Setup EXE)
    using Windows SDK signtool.exe with SHA-256 Authenticode & RFC3161 Timestamping.
    Operates as a graceful no-op when WINDOWS_CERT_PFX_BASE64 is not configured.
#>
[CmdletBinding()]
param(
    [string]$BundledDir = "target/bundled",
    [string]$ReleaseDir = "target/release"
)

$ErrorActionPreference = "Stop"

$certB64 = $env:WINDOWS_CERT_PFX_BASE64
$certPass = $env:WINDOWS_CERT_PASSWORD

if ([string]::IsNullOrWhiteSpace($certB64)) {
    Write-Host "[TEPUI SIGN] WINDOWS_CERT_PFX_BASE64 no configurado; omitiendo firma Authenticode opcional."
    exit 0
}

$signtool = Get-Command "signtool.exe" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1
if (-not $signtool) {
    $sdkRoots = @(
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin",
        "${env:ProgramFiles}\Windows Kits\10\bin"
    )
    foreach ($root in $sdkRoots) {
        if (Test-Path $root) {
            $found = Get-ChildItem -Path $root -Filter "signtool.exe" -Recurse -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match "x64" } |
                Sort-Object FullName -Descending |
                Select-Object -ExpandProperty FullName -First 1
            if ($found) {
                $signtool = $found
                break
            }
        }
    }
}

if (-not $signtool) {
    Write-Warning "[TEPUI SIGN] No se encontro signtool.exe en el entorno; omitiendo firma."
    exit 0
}

$tempPfx = Join-Path $env:TEMP ("tepui_codesign_" + [guid]::NewGuid().ToString("N") + ".pfx")
try {
    $pfxBytes = [Convert]::FromBase64String($certB64)
    [IO.File]::WriteAllBytes($tempPfx, $pfxBytes)

    $targets = @(
        (Join-Path $BundledDir "TEPUI.vst3/Contents/x86_64-win/TEPUI.vst3"),
        (Join-Path $BundledDir "TEPUI.clap"),
        (Join-Path $ReleaseDir "tepui_studio.exe"),
        (Join-Path $BundledDir "TEPUI-Setup-Windows-x64.exe")
    )

    foreach ($bin in $targets) {
        if (Test-Path $bin) {
            Write-Host "[TEPUI SIGN] Firmando con Authenticode SHA-256: $bin"
            $signArgs = @(
                "sign",
                "/f", $tempPfx,
                "/fd", "SHA256",
                "/tr", "http://timestamp.digicert.com",
                "/td", "SHA256",
                "/d", "TEPUI Gran Sabana Sonic Lab",
                $bin
            )
            if (-not [string]::IsNullOrEmpty($certPass)) {
                $signArgs = @(
                    "sign",
                    "/f", $tempPfx,
                    "/p", $certPass,
                    "/fd", "SHA256",
                    "/tr", "http://timestamp.digicert.com",
                    "/td", "SHA256",
                    "/d", "TEPUI Gran Sabana Sonic Lab",
                    $bin
                )
            }
            & $signtool @signArgs
            if ($LASTEXITCODE -ne 0) {
                throw "signtool fallo al firmar $bin (Exit code: $LASTEXITCODE)"
            }
        }
    }
} finally {
    if (Test-Path $tempPfx) {
        Remove-Item -Path $tempPfx -Force -ErrorAction SilentlyContinue
    }
}

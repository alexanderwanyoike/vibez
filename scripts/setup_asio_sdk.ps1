param(
    [string]$Destination = (Join-Path (Split-Path -Parent $PSScriptRoot) ".asio-sdk")
)

$ErrorActionPreference = "Stop"
$SdkUrl = "https://www.steinberg.net/asiosdk"
$SdkSha256 = "d5ebf0c20dd2c5f43771fd0c1418f4b361bf52434ee670097cfa6b3a335e2eca"
$Marker = Join-Path $Destination ".vibez-source-sha256"

function Test-PinnedSdk {
    return (Test-Path (Join-Path $Destination "common/asio.h")) `
        -and (Test-Path (Join-Path $Destination "host/asiodrivers.cpp")) `
        -and (Test-Path (Join-Path $Destination "LICENSE.txt")) `
        -and (Test-Path $Marker) `
        -and ((Get-Content $Marker -Raw).Trim() -eq $SdkSha256)
}

if (-not (Test-PinnedSdk)) {
    if (Test-Path $Destination) {
        Remove-Item -LiteralPath $Destination -Recurse -Force
    }

    $Token = [Guid]::NewGuid().ToString("N")
    $Archive = Join-Path ([IO.Path]::GetTempPath()) "vibez-asio-$Token.zip"
    $Extracted = Join-Path ([IO.Path]::GetTempPath()) "vibez-asio-$Token"
    try {
        Invoke-WebRequest -Uri $SdkUrl -OutFile $Archive
        $ActualSha256 = (Get-FileHash -Path $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($ActualSha256 -ne $SdkSha256) {
            throw "ASIO SDK checksum mismatch. Expected $SdkSha256, received $ActualSha256."
        }

        Expand-Archive -Path $Archive -DestinationPath $Extracted -Force
        $SdkRoot = Get-ChildItem -Path $Extracted -Directory |
            Where-Object { Test-Path (Join-Path $_.FullName "common/asio.h") } |
            Select-Object -First 1
        if ($null -eq $SdkRoot) {
            throw "The pinned ASIO SDK archive does not contain common/asio.h."
        }

        $License = Get-Content (Join-Path $SdkRoot.FullName "LICENSE.txt") -Raw
        if ($License -notmatch "General Public License \(GPL\) Version 3") {
            throw "The ASIO SDK archive does not contain the expected GPLv3 licence option."
        }

        Move-Item -LiteralPath $SdkRoot.FullName -Destination $Destination
        Set-Content -Path $Marker -Value $SdkSha256 -NoNewline
    }
    finally {
        Remove-Item -LiteralPath $Archive -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $Extracted -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$env:CPAL_ASIO_DIR = $Destination
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"

if ($env:GITHUB_ENV) {
    "CPAL_ASIO_DIR=$env:CPAL_ASIO_DIR" | Out-File -FilePath $env:GITHUB_ENV -Append
    "LIBCLANG_PATH=$env:LIBCLANG_PATH" | Out-File -FilePath $env:GITHUB_ENV -Append
}

Write-Host "Using pinned Steinberg ASIO SDK at $Destination"

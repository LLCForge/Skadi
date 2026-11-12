# Skadi launcher for Windows.
#
#   irm https://raw.githubusercontent.com/LLCForge/Skadi/main/install.ps1 | iex
#
# Downloads the Skadi binary for this PC from GitHub Releases into a private temporary folder,
# checks its SHA-256 checksum, also checks its signed build provenance when the GitHub CLI (gh)
# is installed, runs it, and deletes it afterwards. Nothing is installed.
#
# $env:SKADI_VERSION = 'v1.2.3'   use a specific release instead of the latest one

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
# Windows PowerShell 5.1 may default to older TLS versions.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$repo = 'LLCForge/Skadi'
$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
switch ($arch) {
    'X64' { $target = 'x86_64-pc-windows-msvc' }
    'Arm64' { $target = 'aarch64-pc-windows-msvc' }
    default { throw "skadi: unsupported processor: $arch" }
}
$asset = "skadi-$target.exe"

if ($env:SKADI_VERSION) {
    if ($env:SKADI_VERSION -notmatch '^v[0-9][0-9A-Za-z.\-]*$') { throw 'skadi: SKADI_VERSION must look like v1.2.3' }
    $base = "https://github.com/$repo/releases/download/$($env:SKADI_VERSION)"
} else {
    $base = "https://github.com/$repo/releases/latest/download"
}

# A random folder under the user's own temp directory, readable only by this user by default.
$dir = Join-Path ([IO.Path]::GetTempPath()) ("skadi-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $dir | Out-Null
try {
    $exe = Join-Path $dir 'skadi.exe'
    $sums = Join-Path $dir 'SHA256SUMS'
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $exe
    Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile $sums

    $expected = $null
    foreach ($line in Get-Content -LiteralPath $sums) {
        $parts = $line -split '\s+', 2
        if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $asset) { $expected = $parts[0].ToLowerInvariant() }
    }
    if (-not $expected) { throw "skadi: no checksum for $asset" }
    $actual = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw 'skadi: checksum mismatch; the download was not run' }

    if (Get-Command gh -ErrorAction SilentlyContinue) {
        & gh attestation verify $exe --repo $repo *> $null
        if ($LASTEXITCODE -ne 0) { throw 'skadi: build provenance check failed; the download was not run' }
        Write-Host 'skadi: checksum and signed build provenance verified'
    } else {
        Write-Host 'skadi: checksum verified (install the GitHub CLI to also verify the signed build provenance)'
    }

    & $exe @args
} finally {
    Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
}

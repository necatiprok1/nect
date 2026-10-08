# Nect installer for Windows.
#
#   irm https://github.com/necatiprok1/nect/releases/latest/download/install.ps1 | iex
#
# Or, to run it from a file you have read first:
#
#   .\install.ps1
#
# Options:
#
#   -InstallDir <path>   where the binary goes   (default: %LOCALAPPDATA%\Nect\bin)
#   -Version <tag>       a release tag to pin     (default: latest)
#   -Full                install the `full` build (default: the lean build)
#   -NoPath              do not add the directory to the user PATH
#
# The lean build is the language: about 3 MB, no GUI toolkit, no HTTP stack, no
# database engine. The full build adds the language server, the package manager,
# and the FFI. Both run the same programs; they differ in which optional
# built-ins exist. See docs\KURULUM.md.

[CmdletBinding()]
param(
    [string] $InstallDir,
    [string] $Version,
    [switch] $Full,
    [switch] $NoPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Repo = 'necatiprok1/nect'
# Older Windows PowerShell defaults may not negotiate GitHub's required TLS.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

function Say  { param([string] $Message) Write-Host $Message }

# Deliberately not `Write-Error`: with `$ErrorActionPreference = 'Stop'` that
# throws, and a throw inside the checksum block would be caught by the very
# `catch` that exists to report a *missing* SHA256SUMS. A failed verification has
# to end the script, not be reported as a fetch problem.
function Die  { param([string] $Message) [Console]::Error.WriteLine("error: $Message"); exit 1 }

# --- what to fetch ----------------------------------------------------------

# The archive name carries no version, so the "latest" URL is stable across
# releases. The version comes back in the archive and from SHA256SUMS.
$arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($arch -ne 'AMD64') {
    Die "Unsupported architecture: $arch. x64 Windows is the only published target; build from source instead (docs\KURULUM.md)."
}

$suffix = if ($Full) { '-full' } else { '' }
$archive = "nect-x86_64-pc-windows-msvc$suffix.zip"

if ($Version) {
    $label = 'v' + ($Version -replace '^v', '')
    $base = "https://github.com/$Repo/releases/download/$label"
} else {
    $base = "https://github.com/$Repo/releases/latest/download"
    $label = 'latest'
}

Say "Nect ($label, $(if ($Full) { 'full' } else { 'lean' }) build) for x86_64-pc-windows-msvc"

# Windows PowerShell 5.1 ships `Invoke-WebRequest` without `-UseBasicParsing`
# meaning anything useful, and the default parser cannot handle some responses.
# `-UseBasicParsing` is the portable choice across 5.1 and 7.
function Get-Url {
    param([string] $Uri, [string] $OutFile)
    $progress = $ProgressPreference
    # The progress bar writes control characters that corrupt a piped install.
    $ProgressPreference = 'SilentlyContinue'
    try {
        Invoke-WebRequest -Uri $Uri -OutFile $OutFile -UseBasicParsing
    } finally {
        $ProgressPreference = $progress
    }
}

# --- download ---------------------------------------------------------------

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("nect-install-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp -Force | Out-Null
try {
    $archivePath = Join-Path $tmp $archive
    Say "Downloading $base/$archive"
    Get-Url -Uri "$base/$archive" -OutFile $archivePath

    # Do not install an unverified binary, even if the manifest is unavailable.

    $sumsPath = Join-Path $tmp 'SHA256SUMS'
    $expected = $null
    try {
        Get-Url -Uri "$base/SHA256SUMS" -OutFile $sumsPath
        foreach ($line in Get-Content $sumsPath) {
            $parts = $line -split '\s+', 2
            if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $archive) {
                $expected = $parts[0]
                break
            }
        }
    } catch {
        throw 'SHA256SUMS could not be fetched; refusing an unverified installation.'
    }
    if (-not $expected) {
        throw "SHA256SUMS has no entry for $archive."
    }
    $actual = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected.ToLowerInvariant()) {
        throw 'Checksum mismatch: the download is corrupt or tampered with.'
    }
    Say 'Checksum verified.'

    # --- unpack -------------------------------------------------------------

    $unpack = Join-Path $tmp 'unpack'
    Expand-Archive -Path $archivePath -DestinationPath $unpack -Force
    $binary = Join-Path $unpack "nect-x86_64-pc-windows-msvc$suffix\nect.exe"
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "The archive did not contain 'nect.exe'."
    }
    $versionLine = & $binary --version
    if ($LASTEXITCODE -ne 0) {
        throw 'The downloaded binary cannot run on this system.'
    }

    # --- install ------------------------------------------------------------

    if (-not $InstallDir) {
        # LOCALAPPDATA is normally always set, but a stripped service account can
        # lack it, and a null path would silently install to the working
        # directory.
        $baseDir = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { Join-Path $HOME '.nect' }
        $InstallDir = Join-Path $baseDir 'Nect\bin'
    }
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

    # Stage before replacing so a failed copy does not delete an existing install.
    $target = Join-Path $InstallDir 'nect.exe'
    $staged = Join-Path $InstallDir ('.nect-install-' + [guid]::NewGuid() + '.exe')
    $stale = $null
    try {
        Copy-Item -LiteralPath $binary -Destination $staged
        if (Test-Path -LiteralPath $target) {
            $stale = "$target.$([guid]::NewGuid().ToString('N')).old"
            Move-Item -LiteralPath $target -Destination $stale
        }
        try {
            Move-Item -LiteralPath $staged -Destination $target
        } catch {
            if ($stale) { Move-Item -LiteralPath $stale -Destination $target }
            throw
        }
        if ($stale) { Remove-Item -LiteralPath $stale -Force -ErrorAction SilentlyContinue }
    } finally {
        Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue
    }

    Say ''
    Say "Installed: $target"
    if ($versionLine) { Say "           $($versionLine | Select-Object -First 1)" }

    # --- PATH ---------------------------------------------------------------
    #
    # The point of doing this automatically is that the next command works
    # without the user editing anything. The user PATH is edited, not the system
    # PATH, so no elevation is needed and nothing else on the machine changes.

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $onPath = $userPath -and (($userPath -split ';') | Where-Object { $_.TrimEnd('\') -ieq $InstallDir.TrimEnd('\') })

    if (-not $NoPath -and -not (($env:Path -split ';') | Where-Object { $_.TrimEnd('\') -ieq $InstallDir.TrimEnd('\') })) {
        $env:Path = "$InstallDir;$env:Path"
    }
    if ($NoPath) {
        Say ''
        Say '-NoPath was given, so PATH was left alone. Make sure it contains:'
        Say "    $InstallDir"
    } elseif ($onPath) {
        Say ''
        Say 'That directory is already on your PATH.'
    } else {
        $newPath = if ([string]::IsNullOrWhiteSpace($userPath)) { $InstallDir } else { "$userPath;$InstallDir" }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')

        Say ''
        Say "Added $InstallDir to your PATH."
        Say 'Open a new terminal for it to take effect.'
    }

    # --- verify -------------------------------------------------------------

    Say ''
    if (Get-Command nect -ErrorAction SilentlyContinue) {
        Say "Run 'nect --version' to verify, and 'nect run hello.nct' to try it."
    } else {
        Say "Run '$target --version' to verify."
    }
} finally {
    Remove-Item -Path $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

# Installs horizon-server (docs/server.md) on Windows: the current Horizon Nightly (the only build Horizon ships;
# GitHub's latest release while there is none), downloaded from GitHub Releases and checked against the release's
# signed SHA256SUMS when OpenSSL 3 is found (docs/release-flow.md, "Signed checksums"), else against the SHA-256
# GitHub publishes for it. OpenSSL is looked for on the PATH and in Git for Windows, which ships it.
#
#   irm https://raw.githubusercontent.com/horizon-rust/horizon/main/packaging/server/install.ps1 | iex
#   & ([scriptblock]::Create((irm .../install.ps1))) -Version v0.5.0 -Dir D:\Horizon
#
# Releases go to <Dir>\releases\<version>, <Dir>\current points at the one in use, and <Dir>\bin (added to
# your PATH) holds horizon-server.cmd and horizon-prepare.cmd. The game files you prepare from your own
# Minecraft stay in <Dir>\game-files across updates. Nothing of Mojang's is downloaded. No admin rights.
param(
    [string]$Version = "latest",
    [string]$Dir = (Join-Path $env:LOCALAPPDATA "Horizon")
)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Repo = "horizon-rust/horizon"
# Horizon's release public keys, base64: copy of crates/horizon-release-sign/release-keys.txt (rotation adds a key
# there first; keep the two in step, and install.sh's RELEASE_KEYS too).
$ReleaseKeys = @("MU4oiIQcCxkZvbwSjjXs7SySCuM6ywRQ3nGaJYZG0gw=")
$Fallback = $null
$Architecture = $env:PROCESSOR_ARCHITEW6432
if (-not $Architecture) { $Architecture = $env:PROCESSOR_ARCHITECTURE }
switch ($Architecture) {
    "AMD64" { $Asset = "horizon-windows-x64.zip" }
    # A release without an ARM64 build installs the x64 one, which Windows runs under emulation.
    "ARM64" { $Asset = "horizon-windows-arm64.zip"; $Fallback = "horizon-windows-x64.zip" }
    default { throw "No Horizon release for Windows on $Architecture yet: build it from source (docs/server.md)" }
}

function Get-Release([string]$Path) {
    Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/$Path" -Headers @{ Accept = "application/vnd.github+json" }
}
if ($Version -eq "latest") {
    try { $Release = Get-Release "tags/nightly" } catch { $Release = Get-Release "latest" }
} else {
    $Release = Get-Release "tags/$Version"
}
$Tag = $Release.tag_name
$RawTag = $Tag
# A Nightly installs under its date and commit, as `horizon-server update` names it:
# "Nightly 2026-10-28 <middle dot> 0c02067" is nightly-2026-10-28-0c02067 ([char]0xB7: this file stays ASCII).
# Its signature is checked for exactly that version, so the title must name it (as
# horizon_release_sign::nightly_title_version reads it, strictly).
$Nightly = $null
if ($Tag -eq "nightly" -or $Tag.StartsWith("nightly-")) {
    if ($Release.name -cnotmatch "^Nightly (\d{4}-\d{2}-\d{2}) $([char]0xB7) ([0-9a-f]{7,40})$") {
        throw "This Nightly's title doesn't name its date and commit as Horizon signs them, so it can't be checked: not installed"
    }
    $Nightly = "nightly-$($Matches[1])-$($Matches[2])"
    $Tag = $Nightly
}
$Item = $Release.assets | Where-Object { $_.name -eq $Asset } | Select-Object -First 1
if (-not $Item -and $Fallback) {
    Write-Host "Release $Tag has no ${Asset}: installing $Fallback (x64, under emulation)"
    $Asset = $Fallback
    $Item = $Release.assets | Where-Object { $_.name -eq $Asset } | Select-Object -First 1
}
if (-not $Item) { throw "Release $Tag has no $Asset" }
$Digest = $null
if ($Item.digest -and $Item.digest.StartsWith("sha256:")) { $Digest = $Item.digest.Substring(7) }

# The first OpenSSL 3 on the PATH or in Git for Windows (its usr\bin and mingw64\bin), else what was found instead.
function Find-OpenSsl {
    $Candidates = @()
    $OnPath = Get-Command openssl -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($OnPath) { $Candidates += $OnPath.Source }
    $Git = Get-Command git -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($Git) {
        # git.exe is in Git\cmd, Git\bin or Git\mingw64\bin.
        $Folder = Split-Path $Git.Source
        foreach ($Up in 1, 2) {
            $Folder = Split-Path $Folder
            if (-not $Folder) { break }
            foreach ($Sub in "usr\bin\openssl.exe", "mingw64\bin\openssl.exe") {
                $Path = Join-Path $Folder $Sub
                if (Test-Path -LiteralPath $Path -PathType Leaf) { $Candidates += $Path }
            }
        }
    }
    $Other = $null
    foreach ($Path in $Candidates) {
        $ErrorActionPreference = "Continue"
        $Said = (& $Path version 2>$null | Select-Object -First 1)
        if ("$Said" -match "^OpenSSL ([3-9]|[1-9][0-9])\.") { return @{ Path = $Path } }
        if (-not $Other -and $Said) { $Other = ("$Said" -split " ")[0..1] -join " " }
    }
    return @{ Path = $null; Other = $Other }
}

$Tmp = Join-Path ([IO.Path]::GetTempPath()) ("horizon-install-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    # The signature (docs/release-flow.md, "Signed checksums"): SHA256SUMS.sig holds one base64 Ed25519 signature
    # per line over "horizon release <tag>\n" + SHA256SUMS. A Nightly is checked against SHA256SUMS.v2.sig instead,
    # signed for its own version (nightly-<date>-<commit>, from its title), never the channel's SHA256SUMS.sig: an
    # older Nightly can't be passed off as the current one (docs/security.md, "Nightly signatures"). Windows can't
    # check Ed25519 by itself, so this needs OpenSSL 3; without it the install says so and keeps GitHub's SHA-256.
    $Expect = $Digest
    if ($Nightly) { $SigTag = $Nightly; $SigFile = "SHA256SUMS.v2.sig" } else { $SigTag = $RawTag; $SigFile = "SHA256SUMS.sig" }
    $OpenSsl = Find-OpenSsl
    if (-not $OpenSsl.Path -and $OpenSsl.Other) {
        Write-Host "Note: $($OpenSsl.Other) can't check Ed25519 signatures (OpenSSL 3 is needed), so the release signature was not checked; checking GitHub's SHA-256 only."
    } elseif (-not $OpenSsl.Path) {
        Write-Host "Note: OpenSSL isn't installed (Git for Windows includes it), so the release signature was not checked; checking GitHub's SHA-256 only."
    } else {
        $SumsItem = $Release.assets | Where-Object { $_.name -eq "SHA256SUMS" } | Select-Object -First 1
        $SigItem = $Release.assets | Where-Object { $_.name -eq $SigFile } | Select-Object -First 1
        if ($Nightly -and (-not $SumsItem -or -not $SigItem)) {
            throw "$Tag has no $SigFile (its signature for this exact build): it was published before Nightlies were signed for their own version, so it can't be checked. Install the current Nightly (-Version latest) instead."
        } elseif (-not $SumsItem -or -not $SigItem) {
            Write-Host "Note: release $Tag has no signed checksums (it was published before signing began), so the signature was not checked; checking GitHub's SHA-256 only."
        } else {
            $Sums = Join-Path $Tmp "SHA256SUMS"
            $Sigs = Join-Path $Tmp "SHA256SUMS.sig"
            Invoke-WebRequest -UseBasicParsing -Uri $SumsItem.browser_download_url -OutFile $Sums
            Invoke-WebRequest -UseBasicParsing -Uri $SigItem.browser_download_url -OutFile $Sigs
            $Message = Join-Path $Tmp "message"
            [IO.File]::WriteAllBytes($Message, [byte[]]([Text.Encoding]::ASCII.GetBytes("horizon release $SigTag`n") + [IO.File]::ReadAllBytes($Sums)))
            $KeyDer = Join-Path $Tmp "key.der"
            $Signature = Join-Path $Tmp "signature"
            $Signed = $false
            foreach ($Key in $ReleaseKeys) {
                # The raw 32-byte key in an Ed25519 SubjectPublicKeyInfo, the form OpenSSL reads.
                $Prefix = [byte[]](0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00)
                [IO.File]::WriteAllBytes($KeyDer, [byte[]]($Prefix + [Convert]::FromBase64String($Key)))
                foreach ($Line in [IO.File]::ReadAllLines($Sigs)) {
                    if (-not $Line.Trim()) { continue }
                    try { [IO.File]::WriteAllBytes($Signature, [Convert]::FromBase64String($Line.Trim())) } catch { continue }
                    $ErrorActionPreference = "Continue"
                    & $OpenSsl.Path pkeyutl -verify -rawin -pubin -keyform DER -inkey $KeyDer -in $Message -sigfile $Signature 2>&1 | Out-Null
                    if ($LASTEXITCODE -eq 0) { $Signed = $true }
                    $ErrorActionPreference = "Stop"
                }
            }
            if (-not $Signed) {
                throw "$Tag's SHA256SUMS signature doesn't match Horizon's release key: not installed. The download may have been tampered with; please report it on Discord or GitHub."
            }
            $Expect = $null
            foreach ($Line in [IO.File]::ReadAllLines($Sums)) {
                if ($Line -match "^([0-9a-fA-F]{64}) [ *](.+)$" -and $Matches[2] -eq $Asset) { $Expect = $Matches[1]; break }
            }
            if (-not $Expect) { throw "The signed SHA256SUMS doesn't list ${Asset}: not installed" }
            Write-Host "Release signature OK (Horizon release key)."
        }
    }
    if (-not $Expect) { throw "Release $Tag publishes no SHA-256 for $Asset" }

    Write-Host "Downloading Horizon $Tag ($Asset)..."
    $Zip = Join-Path $Tmp $Asset
    Invoke-WebRequest -Uri $Item.browser_download_url -OutFile $Zip
    $Got = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLowerInvariant()
    if ($Got -ne $Expect.ToLowerInvariant()) {
        throw "$Asset doesn't match its SHA-256 (got $Got, expected $Expect): not installed"
    }
    $Unpacked = Join-Path $Tmp "x"
    Expand-Archive -Path $Zip -DestinationPath $Unpacked
    $Server = Get-ChildItem -Path $Unpacked -Recurse -Filter "horizon-server.exe" | Select-Object -First 1
    if (-not $Server) { throw "$Asset holds no horizon-server.exe (a release from before it existed?)" }

    $Releases = Join-Path $Dir "releases"
    $GameFiles = Join-Path $Dir "game-files\horizon-client"
    $Bin = Join-Path $Dir "bin"
    New-Item -ItemType Directory -Force -Path $Releases, $GameFiles, $Bin | Out-Null
    $Target = Join-Path $Releases $Tag
    if (Test-Path $Target) { Remove-Item -Recurse -Force $Target }
    Move-Item -Path $Server.Directory.FullName -Destination $Target
    # Prepared game files live outside the release, so updates and rollbacks keep them. Junctions need no
    # admin rights, unlike symbolic links.
    $Local = Join-Path $Target ".local"
    New-Item -ItemType Directory -Force -Path $Local | Out-Null
    New-Item -ItemType Junction -Path (Join-Path $Local "horizon-client") -Target $GameFiles | Out-Null
    $Current = Join-Path $Dir "current"
    if (Test-Path $Current) { (Get-Item $Current).Delete() }
    New-Item -ItemType Junction -Path $Current -Target $Target | Out-Null
    foreach ($Tool in "horizon-server", "horizon-prepare") {
        Set-Content -Path (Join-Path $Bin "$Tool.cmd") -Encoding ASCII -Value "@`"$Current\$Tool.exe`" %*"
    }
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not (($UserPath -split ";") -contains $Bin)) {
        [Environment]::SetEnvironmentVariable("Path", ($UserPath.TrimEnd(";") + ";" + $Bin).TrimStart(";"), "User")
        Write-Host "Added $Bin to your PATH: open a new terminal to use it."
    }
    Write-Host "Installed Horizon $Tag in $Target."
    if (-not (Get-ChildItem -Path $GameFiles -ErrorAction SilentlyContinue)) {
        Write-Host "Next, prepare the game files from your own Minecraft 26.3 (it needs Java 25 and the game installed"
        Write-Host "once by the official launcher; see docs/server.md):"
        Write-Host "  horizon-prepare --prepare --server-jar C:\path\to\server.jar"
    }
    Write-Host "Then set up a server: mkdir my-server; cd my-server; horizon-server init"
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}

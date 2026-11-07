# PURPOSE: proves packaging/server/install.ps1 checks the release signature (docs/release-flow.md, "Signed checksums").
# WHY: the first install on Windows used to trust only the SHA-256 GitHub publishes (docs/security.md).
# REFS: tests/install_sh_signature.sh (the same cases for install.sh), tools/ci/sign_release.sh (the format signed here).
# LOGS: prints one ok/FAIL line per case; exit 1 when any case fails.
# READS-BY: tests/test_install_ps1_signature.py (unittest wrapper), by hand: pwsh -File tests/install_ps1_signature.ps1
# ENDS-WHEN: it ends by itself; leaves nothing outside its own temporary folder.
# Windows only, Windows PowerShell 5.1 or PowerShell 7. Needs OpenSSL 3 (to sign the throwaway releases); skips
# (exit 77) without it. Invoke-RestMethod and Invoke-WebRequest are replaced by fakes that serve local files, and the
# user's PATH is never written, so nothing touches the network or the account.
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Root = Split-Path -Parent $PSScriptRoot
$Installer = Join-Path $Root "packaging\server\install.ps1"

function Find-TestOpenSsl {
    $Paths = @()
    $OnPath = Get-Command openssl -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($OnPath) { $Paths += $OnPath.Source }
    $Git = Get-Command git -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($Git) { $Paths += Join-Path (Split-Path (Split-Path $Git.Source)) "usr\bin\openssl.exe" }
    foreach ($Path in $Paths) {
        if (-not (Test-Path -LiteralPath $Path)) { continue }
        if ("$(& $Path version 2>$null)" -match "^OpenSSL [3-9]\.") { return $Path }
    }
    return $null
}
$Ssl = Find-TestOpenSsl
if (-not $Ssl) { Write-Output "skip: OpenSSL 3 needed"; exit 77 }

$W = Join-Path ([IO.Path]::GetTempPath()) ("horizon-ps1-test-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $W | Out-Null
# The installer always picks the x64 archive, the one these releases hold, whatever this machine is.
$env:PROCESSOR_ARCHITECTURE = "AMD64"
$env:PROCESSOR_ARCHITEW6432 = $null
$SavedPath = $env:PATH
$Asset = "horizon-windows-x64.zip"
$RealKey = "MU4oiIQcCxkZvbwSjjXs7SySCuM6ywRQ3nGaJYZG0gw="
$script:Fails = 0
$global:HorizonTestRel = $null

# The user's PATH stays as it is: the copies of install.ps1 write through this class instead of [Environment].
Add-Type -TypeDefinition @"
public static class FakeEnvironment {
    public static string GetEnvironmentVariable(string name, string scope) { return ""; }
    public static void SetEnvironmentVariable(string name, string value, string scope) { }
}
"@

# The fakes: the API answers with the case's release, any download with the case's file of that name.
function Invoke-RestMethod { param($Uri, $Headers) $global:HorizonTestRel.Release }
function Invoke-WebRequest {
    param($Uri, $OutFile, [switch]$UseBasicParsing)
    $Source = Join-Path $global:HorizonTestRel.Files ($Uri -split "/")[-1]
    if (-not (Test-Path -LiteralPath $Source)) { throw "404 $Uri" }
    Copy-Item -LiteralPath $Source -Destination $OutFile
}

function Write-Ascii([string]$Path, [string]$Text) { [IO.File]::WriteAllText($Path, $Text, [Text.Encoding]::ASCII) }
function Get-Sha([string]$Path) { (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant() }
function Get-Title([string]$Tag, [string]$Also) {
    if ($Also) { "Nightly $($Also.Substring(8, 10)) $([char]0xB7) $($Also.Substring(19))" } else { $Tag }
}

# One base64 signature line over "horizon release <tag>\n" + SHA256SUMS, as tools/ci/sign_release.sh writes it.
function New-SignatureLine([string]$Dir, [string]$Tag) {
    $Message = Join-Path $Dir "message"
    [IO.File]::WriteAllBytes($Message, [byte[]]([Text.Encoding]::ASCII.GetBytes("horizon release $Tag`n") + [IO.File]::ReadAllBytes((Join-Path $Dir "files\SHA256SUMS"))))
    & $Ssl pkeyutl -sign -rawin -inkey (Join-Path $Dir "key") -keyform DER -in $Message -out (Join-Path $Dir "sig.bin")
    [Convert]::ToBase64String([IO.File]::ReadAllBytes((Join-Path $Dir "sig.bin"))) + "`n"
}

# A fake release: a zip holding horizon-server.exe, signed for Tag with a throwaway key. A Nightly (Tag nightly) is
# also signed for its own version Also (nightly-<date>-<commit>) in SHA256SUMS.v2.sig, titled as nightly-build.yml
# titles it.
function New-Release([string]$Dir, [string]$Tag, [string]$Also = "") {
    New-Item -ItemType Directory -Force -Path (Join-Path $Dir "src\horizon-server-x"), (Join-Path $Dir "files") | Out-Null
    Write-Ascii (Join-Path $Dir "src\horizon-server-x\horizon-server.exe") "fake"
    Compress-Archive -Path (Join-Path $Dir "src\horizon-server-x") -DestinationPath (Join-Path $Dir "files\$Asset")
    Write-Ascii (Join-Path $Dir "files\SHA256SUMS") "$(Get-Sha (Join-Path $Dir "files\$Asset"))  $Asset`n"
    & $Ssl genpkey -algorithm ed25519 -outform DER -out (Join-Path $Dir "key")
    & $Ssl pkey -inform DER -in (Join-Path $Dir "key") -pubout -outform DER -out (Join-Path $Dir "pub.der")
    $Pub = [IO.File]::ReadAllBytes((Join-Path $Dir "pub.der"))
    $script:Key = [Convert]::ToBase64String($Pub[($Pub.Length - 32)..($Pub.Length - 1)])
    Write-Ascii (Join-Path $Dir "files\SHA256SUMS.sig") (New-SignatureLine $Dir $Tag)
    if ($Also) { Write-Ascii (Join-Path $Dir "files\SHA256SUMS.v2.sig") (New-SignatureLine $Dir $Also) }
    Set-Release $Dir $Tag $true (Get-Title $Tag $Also)
    Set-Installer $Dir $script:Key
}

# GitHub's release, as Invoke-RestMethod returns it: the archive with its digest, and the checksum files.
function Set-Release([string]$Dir, [string]$Tag, [bool]$Sums, [string]$Title = $Tag) {
    $Assets = @([pscustomobject]@{
        name = $Asset; digest = "sha256:$(Get-Sha (Join-Path $Dir "files\$Asset"))"
        browser_download_url = "https://dl.test/$Asset"
    })
    if ($Sums) {
        foreach ($Name in "SHA256SUMS", "SHA256SUMS.sig", "SHA256SUMS.v2.sig") {
            if (Test-Path -LiteralPath (Join-Path $Dir "files\$Name")) {
                $Assets += [pscustomobject]@{ name = $Name; browser_download_url = "https://dl.test/$Name" }
            }
        }
    }
    $script:Releases[$Dir] = [pscustomobject]@{ tag_name = $Tag; name = $Title; assets = $Assets }
}
$script:Releases = @{}

# install.ps1 with the throwaway public key in place of the release key, and the fake environment.
function Set-Installer([string]$Dir, [string]$Key) {
    $Text = [IO.File]::ReadAllText($Installer)
    $Patched = $Text -replace '(?m)^\$ReleaseKeys = .*$', "`$ReleaseKeys = @(`"$Key`")" -replace '\[Environment\]::', '[FakeEnvironment]::'
    if (-not $Patched.Contains($Key) -or $Patched.Contains("[Environment]::")) { throw "could not patch install.ps1" }
    Write-Ascii (Join-Path $Dir "install.ps1") $Patched
}

# Runs the case's install.ps1 with PATH set to Path; returns @{ Ok; Out }.
function Invoke-Case([string]$Dir, [string]$Path, [string[]]$Arguments = @()) {
    $global:HorizonTestRel = [pscustomobject]@{ Release = $script:Releases[$Dir]; Files = Join-Path $Dir "files" }
    $env:PATH = $Path
    $Ok = $true
    $Out = try {
        & (Join-Path $Dir "install.ps1") -Dir (Join-Path $Dir "inst") @Arguments 6>&1 | Out-String
    } catch { $Ok = $false; $_.Exception.Message }
    $env:PATH = $SavedPath
    @{ Ok = $Ok; Out = "$Out" }
}

function Test-Case([string]$Name, [bool]$WantOk, [string]$Pattern, $Result) {
    if ($Result.Ok -eq $WantOk -and (-not $Pattern -or $Result.Out.Contains($Pattern))) {
        Write-Output "ok   $Name"
    } else {
        Write-Output "FAIL $Name (installed: $($Result.Ok), wanted $WantOk, pattern '$Pattern')"
        Write-Output (($Result.Out -split "`n" | ForEach-Object { "     $_" }) -join "`n")
        $script:Fails++
    }
}
function Test-That([string]$Name, [bool]$Condition) {
    if (-not $Condition) { Write-Output "FAIL $Name"; $script:Fails++ }
}

try {
    # The cases get the OpenSSL 3 found above, alone on their PATH.
    $P = Split-Path $Ssl
    $Empty = Join-Path $W "empty"; New-Item -ItemType Directory -Path $Empty | Out-Null
    $Nightly = "nightly-2026-11-03-0c02067"

    # 1. Good signature, a Nightly: SHA256SUMS.v2.sig signed for the version its title names.
    $d = Join-Path $W "good"; New-Release $d "nightly" $Nightly
    $r = Invoke-Case $d $P; Test-Case "good signature installs (Nightly, signed for its own version)" $true "Release signature OK" $r
    Test-That "nothing installed" (Test-Path (Join-Path $d "inst\current\horizon-server.exe"))
    Test-That "not installed as $Nightly" ($r.Out.Contains("Downloading Horizon $Nightly "))

    # 1b. A Nightly with only the channel's signature (published before v2): refused, never the channel's signature.
    $d = Join-Path $W "channel"; New-Release $d "nightly"; Set-Release $d "nightly" $true (Get-Title "nightly" $Nightly)
    $r = Invoke-Case $d $P; Test-Case "Nightly without SHA256SUMS.v2.sig refused" $false "has no SHA256SUMS.v2.sig" $r
    Test-That "a Nightly without v2 was installed" (-not (Test-Path (Join-Path $d "inst\current")))

    # 1c. An older Nightly's files and signature retitled as a newer one: its v2 signature names the old version.
    $d = Join-Path $W "rollback"; New-Release $d "nightly" "nightly-2026-11-01-1111111"
    Set-Release $d "nightly" $true (Get-Title "nightly" $Nightly)
    $r = Invoke-Case $d $P; Test-Case "older Nightly passed off as a newer one refused" $false "signature doesn't match" $r

    # 1d. A Nightly whose title doesn't name its date and commit: refused, not dated some other way.
    $d = Join-Path $W "title"; New-Release $d "nightly" $Nightly; Set-Release $d "nightly" $true "Nightly"
    $r = Invoke-Case $d $P; Test-Case "Nightly with an unreadable title refused" $false "title doesn't name its date and commit" $r

    # 2. Good signature, a normal tag.
    $d = Join-Path $W "tag"; New-Release $d "v1.2.3"
    $r = Invoke-Case $d $P @("-Version", "v1.2.3"); Test-Case "good signature installs (release tag)" $true "Release signature OK" $r

    # 3. Tampered archive: GitHub's digest is made to agree with the swapped archive, only SHA256SUMS catches it.
    $d = Join-Path $W "arc"; New-Release $d "v1.2.3"
    Add-Content -LiteralPath (Join-Path $d "files\$Asset") -Value "x"; Set-Release $d "v1.2.3" $true
    $r = Invoke-Case $d $P @("-Version", "v1.2.3"); Test-Case "tampered archive refused" $false "doesn't match its SHA-256" $r
    Test-That "tampered archive was installed" (-not (Test-Path (Join-Path $d "inst\current")))

    # 4. Tampered SHA256SUMS (the archive's line swapped for the hash of the tampered archive).
    $d = Join-Path $W "sums"; New-Release $d "v1.2.3"
    Add-Content -LiteralPath (Join-Path $d "files\$Asset") -Value "x"
    Write-Ascii (Join-Path $d "files\SHA256SUMS") "$(Get-Sha (Join-Path $d "files\$Asset"))  $Asset`n"; Set-Release $d "v1.2.3" $true
    $r = Invoke-Case $d $P @("-Version", "v1.2.3"); Test-Case "tampered SHA256SUMS refused" $false "signature doesn't match" $r

    # 5. A signature made for another tag does not pass (a release's list replayed under another tag).
    $d = Join-Path $W "replay"; New-Release $d "v1.2.3"; Set-Release $d "v9.9.9" $true
    $r = Invoke-Case $d $P @("-Version", "v9.9.9"); Test-Case "signature replayed under another tag refused" $false "signature doesn't match" $r

    # 6. Signed by a key that is not the release key.
    $d = Join-Path $W "wrong"; New-Release $d "v1.2.3"; Set-Installer $d $RealKey
    $r = Invoke-Case $d $P @("-Version", "v1.2.3"); Test-Case "wrong key refused" $false "signature doesn't match" $r

    # 7. No OpenSSL: says so, keeps the SHA-256 check.
    $d = Join-Path $W "none"; New-Release $d "v1.2.3"
    $r = Invoke-Case $d $Empty @("-Version", "v1.2.3"); Test-Case "no OpenSSL: installs, says it was not checked" $true "OpenSSL isn't installed" $r
    $d = Join-Path $W "none2"; New-Release $d "v1.2.3"; $script:Releases[$d].assets[0].digest = "sha256:0000"
    $r = Invoke-Case $d $Empty @("-Version", "v1.2.3"); Test-Case "no OpenSSL: a wrong GitHub SHA-256 still refused" $false "doesn't match its SHA-256" $r

    # 8. Old OpenSSL and LibreSSL: says so, keeps the SHA-256 check.
    foreach ($v in "OpenSSL 1.1.1w  11 Sep 2023", "LibreSSL 3.3.6") {
        $d = Join-Path $W ("old-" + $v.Split(" ")[0]); New-Release $d "v1.2.3"
        $Bin = Join-Path $d "old-bin"; New-Item -ItemType Directory -Path $Bin | Out-Null
        Write-Ascii (Join-Path $Bin "openssl.cmd") "@echo $v`r`n"
        $r = Invoke-Case $d $Bin @("-Version", "v1.2.3"); Test-Case "$($v.Split(' ')[0]): installs, says it can't check signatures" $true "can't check Ed25519" $r
    }

    # 9. A release published before signing (no SHA256SUMS): says so, keeps the SHA-256 check.
    $d = Join-Path $W "legacy"; New-Release $d "v1.2.3"; Set-Release $d "v1.2.3" $false
    $r = Invoke-Case $d $P @("-Version", "v1.2.3"); Test-Case "unsigned release: installs, says so" $true "no signed checksums" $r

    # 10. OpenSSL found in Git for Windows, not on the PATH: git.exe in Git\cmd, OpenSSL in Git\usr\bin (or
    # Git\mingw64\bin). Only when this machine's OpenSSL is such a copy, which the fake Git folder links to.
    $Up = Split-Path $P
    if ((Split-Path $P -Leaf) -eq "bin" -and (Split-Path $Up -Leaf) -in "usr", "mingw64") {
        $d = Join-Path $W "git"; New-Release $d "v1.2.3"
        $GitDir = Join-Path $d "Git"; New-Item -ItemType Directory -Path (Join-Path $GitDir "cmd") | Out-Null
        Write-Ascii (Join-Path $GitDir "cmd\git.exe") ""
        New-Item -ItemType Junction -Path (Join-Path $GitDir (Split-Path $Up -Leaf)) -Target $Up | Out-Null
        $r = Invoke-Case $d (Join-Path $GitDir "cmd") @("-Version", "v1.2.3"); Test-Case "OpenSSL from Git for Windows checks the signature" $true "Release signature OK" $r
        (Get-Item (Join-Path $GitDir (Split-Path $Up -Leaf))).Delete()
    } else {
        Write-Output "skip OpenSSL from Git for Windows (this OpenSSL isn't Git's)"
    }
} finally {
    $env:PATH = $SavedPath
    Remove-Item -Recurse -Force $W -ErrorAction SilentlyContinue
}

Write-Output "$($script:Fails) case(s) failed"
if ($script:Fails -ne 0) { exit 1 }
exit 0

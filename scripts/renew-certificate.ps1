<#
.SYNOPSIS
    Renews the public Let's Encrypt *.localbox.ro certificate and, on request,
    deploys it to the FastAdHunter container on bobdenaut.

.DESCRIPTION
    Runs under Windows PowerShell 5.1 (Task Scheduler) and PowerShell 7. Safe to
    run on any cadence: lego decides for itself whether the certificate is due (default
    window is a third of the lifetime, so 30 days on a 90-day certificate) and
    no-ops otherwise. Schedule it weekly - monthly can leave as little as a week
    of margin depending on where the window falls. See
    docs/public-certificate.md.

    The default run never touches the router. Deployment is opt-in via -Deploy
    because it stops and starts the household's only DNS resolver.

    Full background: docs/public-certificate.md

.PARAMETER Deploy
    After a successful renewal, copy the pair to the container's /config and
    restart it. Without this the new certificate stays on the dev box.

.PARAMETER Force
    Pass --renew-force to lego, re-issuing even when not due. Mind the Let's
    Encrypt limit of 5 duplicate certificates per week.

.PARAMETER CheckOnly
    Report days remaining and exit. Touches nothing, contacts no one except the
    live listener. Intended for a daily run that only warns.

.PARAMETER WarnDays
    Exit code 2 when fewer than this many days remain. Default 30.

.EXAMPLE
    .\scripts\renew-certificate.ps1 -CheckOnly

.EXAMPLE
    .\scripts\renew-certificate.ps1 -Deploy

.NOTES
    Exit codes:  0 nothing to do / success
                 2 certificate expiring within -WarnDays (CheckOnly)
                 1 failure
#>
[CmdletBinding()]
param(
    [switch] $Deploy,
    [switch] $Force,
    [switch] $CheckOnly,
    [int]    $WarnDays = 30
)

Set-StrictMode -Version Latest

# 'Continue', not 'Stop'. Every external tool here - openssl, lego, scp, ssh -
# writes progress to stderr, and under 'Stop' PowerShell turns the first such
# line into a terminating error even when the command succeeds. This script
# checks $LASTEXITCODE after every native call and fails explicitly instead.
$ErrorActionPreference = 'Continue'

# -- Settings ----------------------------------------------------------------
$RepoRoot   = Split-Path -Parent $PSScriptRoot
$LegoExe    = Join-Path $HOME 'bin\lego.exe'
$LegoPath   = Join-Path $RepoRoot '.vscode\lego'
$CertDir    = Join-Path $LegoPath 'certificates'
$CertFile   = Join-Path $CertDir '_.localbox.ro.crt'
$KeyFile    = Join-Path $CertDir '_.localbox.ro.key'
$IssuerFile = Join-Path $CertDir '_.localbox.ro.issuer.crt'
$TokenFile  = Join-Path $RepoRoot '.vscode\cloudflare.token'
$LogDir     = Join-Path $RepoRoot '.vscode\lego\logs'
$OpenSslDir = 'C:\Program Files\OpenSSL-Win64\bin'

$Email       = 'liviu.voicu@gmail.com'
$Resolver    = '192.168.10.1:53'
$Chain       = 'ISRG Root X2'
$RouterHost  = 'bobdenaut'
$RemoteDir   = 'kingston/fastadhunter/config'
# The container name carries the version (fastadhunter-0.3.4), so match by
# regex, never by exact name. The quotes are required: with them stripped the
# same find matched no container at all when tested on the router.
$Container   = '[find name~"fastadhunter"]'
$LiveHost    = 'fah-api.localbox.ro'
$LivePort    = 8443

# -- Logging -----------------------------------------------------------------
if (-not (Test-Path $LogDir)) { New-Item -ItemType Directory -Path $LogDir -Force | Out-Null }
$LogFile = Join-Path $LogDir ("renew-{0:yyyy-MM-dd}.log" -f (Get-Date))

function Write-Log {
    param([string] $Message, [string] $Level = 'INFO')
    $line = "{0:yyyy-MM-dd HH:mm:ss} [{1}] {2}" -f (Get-Date), $Level, $Message
    Write-Host $line
    Add-Content -LiteralPath $LogFile -Value $line
}

function Fail {
    param([string] $Message)
    Write-Log $Message 'ERROR'
    exit 1
}

# -- PATH: a Task Scheduler session may predate the OpenSSL install ----------
# winget does not add OpenSSL to PATH; it was appended to the user PATH by hand.
# Re-read the registry so a stale environment block does not break the run.
$env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine').TrimEnd(';') +
            ';' + [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not (Get-Command openssl -ErrorAction SilentlyContinue)) {
    if (Test-Path (Join-Path $OpenSslDir 'openssl.exe')) { $env:Path += ";$OpenSslDir" }
}
if (-not (Get-Command openssl -ErrorAction SilentlyContinue)) {
    Fail 'openssl is not on PATH  -  see docs/public-certificate.md, Shells'
}

# -- Helpers -----------------------------------------------------------------

# Router commands carry double quotes ($Container), and the two PowerShells
# hand them to ssh.exe differently. Windows PowerShell 5.1 (and PowerShell 7 in
# 'Legacy' mode) copies the argument into the command line unescaped, ssh's own
# parser eats the quotes, and the router sees a bare word that matches nothing.
# PowerShell 7.3+ escapes them itself, and escaping twice breaks the command
# the other way. Pick per session, not per script.
$EscapeRouterQuotes = -not (
    (Get-Variable PSNativeCommandArgumentPassing -ErrorAction SilentlyContinue) -and
    $PSNativeCommandArgumentPassing -ne 'Legacy')

# RouterOS exits 0 on its own errors ("no such item", "input does not match"),
# so $LASTEXITCODE only reports ssh failures. Whatever the router prints comes
# back for the caller to judge; a successful stop or start prints nothing.
function Invoke-Router {
    param([string] $Command)
    $arg = if ($EscapeRouterQuotes) { $Command -replace '"', '\"' } else { $Command }
    $out = & ssh -o BatchMode=yes $RouterHost $arg 2>$null
    if ($LASTEXITCODE -ne 0) { Fail "ssh to $RouterHost failed (exit $LASTEXITCODE) running: $Command" }
    ($out | Where-Object { $_ }) -join ' '
}

# The winget OpenSSL build ships no CA bundle, so anything that verifies a
# chain has to borrow the Windows trust store.
$CaStore = @('-CAstore', 'org.openssl.winstore://')

# openssl prints "notAfter=Dec  8 07:46:41 2026 GMT" - two spaces before a
# single-digit day, which ParseExact rejects unless the run is collapsed first.
function ConvertTo-NotAfter {
    param([object] $Raw)
    if (-not $Raw) { return $null }
    $text = ($Raw -join ' ')
    if ($text -notmatch 'notAfter=(.+?)\s*GMT') { return $null }
    $stamp = ($Matches[1] -replace '\s+', ' ').Trim()
    [datetime]::ParseExact(
        $stamp,
        'MMM d HH:mm:ss yyyy',
        [Globalization.CultureInfo]::InvariantCulture,
        [Globalization.DateTimeStyles]::AssumeUniversal -bor
        [Globalization.DateTimeStyles]::AdjustToUniversal)
}

function Get-CertNotAfter {
    param([string] $Path)
    if (-not (Test-Path $Path)) { return $null }
    $text = & openssl x509 -in $Path -noout -enddate 2>$null
    if ($LASTEXITCODE -ne 0) { return $null }
    ConvertTo-NotAfter $text
}

function Get-CertFingerprint {
    param([string] $Path)
    if (-not (Test-Path $Path)) { return $null }
    $out = & openssl x509 -in $Path -noout -fingerprint -sha256 2>$null
    if ($LASTEXITCODE -ne 0) { return $null }
    ($out -split '=')[-1].Trim()
}

function Get-LiveNotAfter {
    $out = '' | & openssl s_client -connect "${LiveHost}:${LivePort}" `
                    -servername $LiveHost @CaStore 2>$null |
           & openssl x509 -noout -enddate 2>$null
    ConvertTo-NotAfter $out
}

# -- CheckOnly ---------------------------------------------------------------
if ($CheckOnly) {
    $live = Get-LiveNotAfter
    if (-not $live) { Fail "could not read a certificate from ${LiveHost}:${LivePort}  -  is the container up?" }
    $days = [int]([math]::Floor(($live - [datetime]::UtcNow).TotalDays))
    Write-Log ("live certificate on {0}:{1} expires {2:yyyy-MM-dd}  -  {3} day(s) left" -f
               $LiveHost, $LivePort, $live, $days)
    if ($days -lt $WarnDays) {
        Write-Log "under the $WarnDays-day threshold; renewal is due" 'WARN'
        exit 2
    }
    exit 0
}

# -- Preflight ---------------------------------------------------------------
Write-Log "renewal run starting (Deploy=$Deploy Force=$Force)"

if (-not (Test-Path $LegoExe))  { Fail "lego not found at $LegoExe" }
if (-not (Test-Path $CertDir))  { Fail "no certificate store at $CertDir" }

# The token file is routinely left empty between renewals. An empty token fails
# part-way through the DNS-01 challenge, after an ACME order is already open,
# which spends one of the five failed validations per hour. Catch it here.
if (-not (Test-Path $TokenFile)) { Fail "no Cloudflare token at $TokenFile" }
$token = (Get-Content -LiteralPath $TokenFile -Raw -ErrorAction SilentlyContinue)
if ($null -eq $token) { $token = '' }
$token = $token.Trim()
if ($token.Length -eq 0) {
    Fail "$TokenFile is empty  -  create a Zone:DNS:Edit token for localbox.ro first (docs/public-certificate.md, step 4)"
}

$before      = Get-CertFingerprint $CertFile
$beforeUntil = Get-CertNotAfter    $CertFile
if ($beforeUntil) {
    Write-Log ("current local certificate expires {0:yyyy-MM-dd} ({1} day(s) left)" -f
               $beforeUntil, [int]([math]::Floor(($beforeUntil - [datetime]::UtcNow).TotalDays)))
}

# -- Renew -------------------------------------------------------------------
# --preferred-chain is not optional: without it lego takes the default Gen Y
# chain, which the OS store trusts and webpki-roots (what the binary uses)
# does not. The result verifies by hand and is refused by the probe.
$legoArgs = @(
    'run',
    '--dns', 'cloudflare',
    '--dns.resolvers', $Resolver,
    '--dns.propagation.wait', '30s',
    '--preferred-chain', $Chain,
    '--domains', '*.localbox.ro',
    '--domains', 'localbox.ro',
    '--email', $Email,
    '--accept-tos',
    '--path', $LegoPath
)
if ($Force) { $legoArgs += '--renew-force' }

$env:CLOUDFLARE_DNS_API_TOKEN = $token
try {
    Write-Log "running lego (a random pre-renewal sleep is normal and may take a while)"
    $legoLog = Join-Path $LogDir ("lego-{0:yyyy-MM-dd-HHmmss}.log" -f (Get-Date))
    # Redirect rather than pipe: piping lego buffers its log until exit, which
    # makes a stall look like normal progress.
    & $LegoExe @legoArgs *> $legoLog
    $legoExit = $LASTEXITCODE
} finally {
    Remove-Item Env:\CLOUDFLARE_DNS_API_TOKEN -ErrorAction SilentlyContinue
}

Get-Content -LiteralPath $legoLog | ForEach-Object { Write-Log $_ 'lego' }
if ($legoExit -ne 0) { Fail "lego exited $legoExit  -  see $legoLog" }

$after      = Get-CertFingerprint $CertFile
$afterUntil = Get-CertNotAfter    $CertFile

if ($before -eq $after) {
    Write-Log 'certificate unchanged  -  not due for renewal yet, nothing to deploy'
    exit 0
}

Write-Log ("renewed: new certificate expires {0:yyyy-MM-dd}" -f $afterUntil)

# -- Verify before it goes anywhere near the router --------------------------
# This is step 8 of docs/public-certificate.md, run in code. It has to be a
# cryptographic path validation against the pinned root, not a text search:
# the string "ISRG Root X2" appears in the wrong chain too (as a subject, with
# ISRG Root X1 as its issuer), so a substring check accepts both and lets the
# exact failure --preferred-chain is supposed to prevent reach the router.
#
# 'openssl verify' with no -CAfile would check the Windows store, which trusts
# roots webpki-roots does not carry, and would answer OK on a certificate the
# binary rejects. Pinning the root is the whole point.
$rootPem = Join-Path $env:TEMP 'isrg-root-x2.pem'
& curl.exe -sfo $rootPem https://letsencrypt.org/certs/isrg-root-x2.pem
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $rootPem)) {
    Fail 'could not fetch the ISRG Root X2 root to verify against - refusing to deploy unverified'
}

$verifyOut = (& openssl verify -CAfile $rootPem -untrusted $IssuerFile $CertFile 2>&1) -join "`n"
if ($LASTEXITCODE -ne 0 -or $verifyOut -notmatch ': *OK') {
    Fail "the new certificate does not chain to '$Chain': $verifyOut - refusing to deploy. Check $legoLog"
}
Write-Log "verified: the new certificate chains to '$Chain' (webpki-roots will accept it)"

if (-not $Deploy) {
    Write-Log 'renewed locally. Re-run with -Deploy to copy it to the router and restart the container.'
    exit 0
}

# -- Deploy: this restarts the household's only DNS resolver -----------------
Write-Log 'deploying to the router  -  the container will restart and DNS will be down briefly' 'WARN'

& scp $CertFile "${RouterHost}:${RemoteDir}/api-cert.pem"
if ($LASTEXITCODE -ne 0) { Fail 'scp of the certificate failed' }
& scp $KeyFile  "${RouterHost}:${RemoteDir}/api-key.pem"
if ($LASTEXITCODE -ne 0) { Fail 'scp of the key failed  -  /config now holds a mismatched pair, fix before restarting' }
Write-Log 'pair copied to /config'

# The loaded pair is fixed for the life of the process: ApiServer::bind builds
# its TlsAcceptor once and there is no resolver to swap. Only a restart picks
# the new certificate up.
$err = Invoke-Router "/container/stop $Container"
if ($err) { Fail "container stop failed: $err" }

# Stopping is asynchronous; a start issued while the container is still going
# down is refused and leaves the resolver off. Wait for running=false first.
$stopDeadline = (Get-Date).AddSeconds(60)
do {
    Start-Sleep -Seconds 3
    $running = Invoke-Router ":put [/container/get $Container running]"
} while ($running -ne 'false' -and (Get-Date) -lt $stopDeadline)
if ($running -ne 'false') {
    Fail "the container did not stop within 60 s (running=$running)  -  check the router now"
}
Write-Log 'container stopped'

$err = Invoke-Router "/container/start $Container"
if ($err) { Fail "container start failed: $err  -  the resolver is down, check the router now" }
Write-Log 'container started'

# -- Confirm the listener actually serves the new certificate ----------------
$deadline = (Get-Date).AddSeconds(120)
do {
    Start-Sleep -Seconds 10
    $live = Get-LiveNotAfter
} while (-not $live -and (Get-Date) -lt $deadline)

if (-not $live) { Fail "the API did not answer on ${LiveHost}:${LivePort} after the restart  -  check the router" }
if ($live.Date -ne $afterUntil.Date) {
    Fail ("the listener still serves a certificate expiring {0:yyyy-MM-dd}, expected {1:yyyy-MM-dd}" -f $live, $afterUntil)
}

Write-Log ("done  -  {0}:{1} now serves the certificate expiring {2:yyyy-MM-dd}" -f $LiveHost, $LivePort, $live)
exit 0

# Public TLS certificate — obtaining and renewing

How `*.localbox.ro` was obtained from Let's Encrypt, and what renewing it needs.
Written from the run of 2026-09-09; every command here is one that was executed,
not a sketch.

## What this is not

This is **not** FastAdHunter's own CA. `fah-certs` runs a private CA that mints
leaf certificates for HTTPS interception, governed by SECURITY.md and ADR-0007;
clients trust it only because the owner installs it. Two different things:

| | Private CA (`fah-certs`) | This certificate |
| --- | --- | --- |
| Issued by | FastAdHunter itself | Let's Encrypt |
| Trusted because | the owner installed it on the client | it chains to a root every OS ships |
| Used for | interception leaves, per-client opt-in | client-facing DoT/DoH, the P3 origin |
| Lifetime | minted per host, cached | 90 days, renewed |

## Why a public certificate is needed at all

Two consumers, one of which cannot be worked around:

- **DoT / DoH listeners (p3-05).** Android's Private DNS validates the server
  certificate against the system store and fails closed. A private CA is
  refused, so a household client cannot use the listeners without a public name.

  The name is **`fah-dot.localbox.ro`** (owner's decision, 2026-09-13) — the
  hostname typed into Android Private DNS. It is the only new name Phase 3
  needs: plain DNS is reached by address through DHCP, and DoH rides the API
  server at `https://fah-api.localbox.ro:8443/dns-query`
  (`fah-api/src/routes.rs:125`). The wildcard already covers it; one label only,
  so `fah-dot.localbox.ro` works and `a.fah-dot.localbox.ro` does not. The
  record is `A → 172.17.0.2`, DNS-only — Cloudflare cannot proxy DoT, which is
  raw TLS rather than HTTPS — and it exists, created 2026-09-13. It resolves
  today and answers nothing: no deployed build binds `:853` yet, so a probe
  before the Phase 3 deploy gets a refused connection rather than a certificate
  error. Reaching it is LAN-only by construction; a phone off the LAN resolves
  a private address and Private DNS fails closed, which is why WAN access is a
  separate decision (dst-nat on 853, or WireGuard) and has not been taken.

  **It only works while no CA exists.** `dot_tls` falls back to the API
  certificate pair, which is this public one, but only when the store holds no
  CA; with a CA present `fah-certs`' `MintingResolver` mints a private leaf for
  the SNI the client sent (`fastadhunter/src/main.rs:970`, the `store.has_ca()`
  branch at `999-1008`). Android always sends SNI in
  hostname mode, so generating a CA — to try interception, say — silently
  breaks Private DNS on every device in the house. Interception ships disabled,
  so the shipped state is the working one.
- **The P3 origin.** The release probe verifies upstreams against
  `webpki-roots` only — `crates/fah-http/src/tls.rs:38`. A self-signed or
  local-CA origin answers `UnknownIssuer`. `--all-features` enables the
  `test-harness` trust-anchor injection, but
  `crates/fah-api/src/lib.rs:18` is a `compile_error!` that forbids that feature
  in `--release`, so the release probe has no escape hatch.

## Current state

| Item | Value |
| ---- | ----- |
| Domain | `localbox.ro` |
| Registrar | NameBox, 36.68 RON first year, **67.62 RON + VAT/year** after, auto-renew on |
| Registry expiry | 2027-09-09 |
| DNS | Cloudflare free plan — `gracie.ns.cloudflare.com`, `fattouche.ns.cloudflare.com` |
| Zone signing | unsigned, no DS at the parent |
| Certificate | `*.localbox.ro` + `localbox.ro`, EC256 |
| Valid | 2026-09-09 → 2026-12-08 |
| Renew after | **2026-11-08** |
| Chain | leaf ← `YE2` ← `Root YE` (cross-signed) ← `ISRG Root X2` |
| Files | `.vscode/lego/` — gitignored, untracked, holds the private key |
| ACME client | `lego 5.4.1` at `~/bin/lego.exe` (not on PATH; call it by full path) |
| Cloudflare token | `.vscode/cloudflare.token` — **empty, 0 bytes as of 2026-09-17**. Renewal cannot run until it is re-created; see §Renewing |
| OpenSSL | `C:\Program Files\OpenSSL-Win64\bin\openssl.exe`, 4.0.2, installed 2026-09-17 via `winget install -e --id ShiningLight.OpenSSL.Light`. Ships **no CA bundle** — see §Shells |

## Shells

The host is **Windows 11**. Every command below is given twice — Git Bash first
(what was actually run), then the PowerShell equivalent. Use either; do not mix
them inside one step.

Windows PowerShell **5.1** is what ships here (`$PSVersionTable.PSVersion`), not
PowerShell 7. The equivalents are written for 5.1 and use nothing newer.

Four differences that produce wrong results rather than errors:

- **`curl` is not curl in PowerShell.** It is an alias for `Invoke-WebRequest`,
  which does not understand `-s`, `--resolve` or `-I`. Always type `curl.exe`.
  That resolves to a **different binary in each shell** — `C:\WINDOWS\system32\
  curl.exe` in PowerShell, `C:\Program Files\Git\mingw64\bin\curl.exe` in Git
  Bash, because Git puts only `Git\cmd` on the system PATH, not `mingw64\bin`.
  Both are 8.21.0 and both are Schannel, so the note below still holds for
  either; the binaries are simply not the same file.
- **`$env:VAR = …` persists for the whole session.** The Bash form
  `VAR=… command` sets the variable for one command only. After running a
  PowerShell step the API token stays in the session's environment until the
  window closes — close it when finished, or `Remove-Item Env:\CLOUDFLARE_DNS_API_TOKEN`.
- **`Get-Content -Raw` returns the file byte-for-byte**, including a trailing
  newline if the editor left one. Cloudflare rejects a token with trailing
  whitespace, so `.Trim()` is kept as the equivalent of `tr -d ' \r\n'` — a
  no-op when the file is clean, and the difference between working and a
  puzzling 401 when it is not.
- **Redirect with `*>`, not `|`.** Piping lego through anything buffers the log
  until the process exits, which is what made a stall look like normal progress.

`whois.exe` (Sysinternals) and `nslookup.exe` (System32) are on PATH in both
shells.

**OpenSSL is a separate install, and it changed on 2026-09-17.** Until that date
the only copy was Git's, at `C:\Program Files\Git\mingw64\bin\openssl.exe`,
reachable from Git Bash and **not** from PowerShell — a PowerShell step calling
`openssl` failed with `The term 'openssl' is not recognized`. It is now
installed natively (ShiningLight 4.0.2, winget) at `C:\Program Files\
OpenSSL-Win64\bin`, appended to the **user** PATH by hand because winget does
not do it. Two consequences:

- A shell opened before that PATH edit will not see it. Re-read the registry
  with `$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') +
  ';' + [Environment]::GetEnvironmentVariable('Path','User')`, or open a new
  window.
- **That build ships no CA bundle.** Every verification below that checks a
  chain must pass `-CAstore 'org.openssl.winstore://'` to borrow the Windows
  trust store; without it OpenSSL reports `unable to get local issuer
  certificate` on a certificate that is perfectly fine, which is exactly the
  failure step 8 exists to detect. Git Bash's OpenSSL 3.5.7 has its own bundle
  and does not need the flag — another reason the two shells are not
  interchangeable here.

## Environment facts the procedure depends on

Both bit during the first attempt. Neither is obvious from any error message.

- **The LAN blocks outbound port 53.** `nslookup … 1.1.1.1` times out;
  `192.168.10.1` answers. Any tool that talks to a public resolver directly
  stalls rather than fails.
- **curl here is Schannel-backed** — `curl -V` reports `Schannel`, in both
  shells, because both run the same Git binary. It ignores `--cacert`. For
  anything that needs a specific trust anchor, use `openssl` or a Node
  `tls.connect` script.

Because of the first, DNS state cannot be read through the local resolver
reliably — it is FastAdHunter, and it caches. Read it over DoH pinned to an
address instead:

```sh
# Git Bash
curl -s --resolve dns.google:443:8.8.8.8 \
  "https://dns.google/resolve?name=localbox.ro&type=NS"
```

```powershell
# PowerShell
curl.exe -s --resolve dns.google:443:8.8.8.8 `
  "https://dns.google/resolve?name=localbox.ro&type=NS"
```

## Procedure

### 1. Register the domain

NameBox, 1 year. Decline everything else:

- **Management DNS (52.43 RON)** — not needed. Cloudflare does it free and
  offers the API that DNS-01 requires.
- **Hosting** — `Fără găzduire`.
- Multi-year is not a discount: every tier is 67.62 RON/year, and only year one
  is promotional, so 1 year + 9 renewals costs less than the 10-year tier.

`.ro` requires a CNP for `Persoană fizică`. That is a ROTLD rule, not a
registrar quirk.

Availability is checked against the registry, not a reseller's page. Replace the
name in the first line with the one being checked:

```sh
# Git Bash
d=localbox.ro
whois "$d" 2>/dev/null | grep -q "No entries found" \
  && echo "$d FREE" || echo "$d TAKEN"
```

```powershell
# PowerShell
$d = 'localbox.ro'
if (whois.exe $d 2>$null | Select-String 'No entries found') { "$d FREE" }
else { "$d TAKEN" }
```

Run today, `localbox.ro` answers `TAKEN` — it is yours. Before 2026-09-09 it
answered `FREE`, which is what made the purchase possible.

The verdict is printed either way. A bare
`whois … | grep "No entries found"` answers a free domain with the match and a
**registered** one with silence — and silence reads like the command failed. It
did not; drop the filter and the record is there, `Domain Name:` and
`Registrar:` and all.

Suppress stderr (`2>/dev/null`, `2>$null`) or Sysinternals `whois` prints
`No such host is known.` before falling back to `RO.whois-servers.net`. That
line is noise from its first lookup, not the answer, and it is the one thing
that *does* survive to the console while the real output is being filtered — so
it looks exactly like the result.

ROTLD rate-limits WHOIS aggressively — a sweep of ~16 names exhausts it for a
while. Note also that a `.ro` domain with **no NS records is still registered**;
absence of NS proves nothing, only WHOIS does.

### 2. Delegate DNS to Cloudflare

`dash.cloudflare.com` → **Add a site** → **Connect a domain** → `localbox.ro` →
**Free**. Cloudflare Registrar does not sell `.ro`, which is irrelevant — it
will run DNS for a domain it did not sell.

Cloudflare refuses to activate a zone with zero records, so add one before
continuing:

| Field | Value |
| ----- | ----- |
| Type | `A` |
| Name | `router` |
| IPv4 | `192.168.10.1` |
| Proxy status | **DNS only** (grey cloud) |

Proxying must be off. It replaces the answer with Cloudflare's addresses and
covers HTTP ports only, so a name pointing at a box on the LAN would resolve to
Cloudflare instead. For a private address Cloudflare forces DNS only by itself
and labels the record `DNS only - reserved`.

Then take the assigned nameserver pair and set it at NameBox under
**Nameservere → Nameservere proprii**, replacing `ns1`/`ns2.namebox.ro`. Leave
slots 3–5 empty. Order does not matter; NS sets are unordered.

Confirm DNSSEC is off at the registrar first. A stale DS record pointing at the
old nameservers makes the zone bogus after the switch and every validating
resolver answers SERVFAIL.

### 3. Verify the delegation before going further

Let's Encrypt reads the authoritative nameservers, not a cache. Until this
flips, DNS-01 cannot succeed:

```sh
# Git Bash
curl -s --resolve dns.google:443:8.8.8.8 \
  "https://dns.google/resolve?name=localbox.ro&type=NS"
```

```powershell
# PowerShell
curl.exe -s --resolve dns.google:443:8.8.8.8 `
  "https://dns.google/resolve?name=localbox.ro&type=NS"
```

Wanted: `"Status":0` and two `.ns.cloudflare.com` entries. While the registry
still holds the old delegation the answer is `"Status":2` with
`Name servers refused query (lame delegation?)` — the old nameservers are still
listed but no longer host the zone. That state is expected and clears itself.
ROTLD published the change in **under 60 seconds**, not the 24 hours both
Cloudflare and NameBox warn about.

Also confirm the zone is unsigned — a `DS` query returning `"Status":0` with
only an SOA in the authority section means no DS exists, so there is no
DNSSEC-bogus failure mode.

### 4. Create a Cloudflare API token

**My Profile → API Tokens → Create Token → Edit zone DNS.** Under Zone
Resources choose **Include → Specific zone → `localbox.ro`**. Not the global
API key, and not all zones.

Save it to `.vscode/cloudflare.token`, one line, nothing else. `.vscode/` is in
`.gitignore` and nothing under it is tracked. Verify with the `verify` endpoint
Cloudflare shows on the confirmation screen; it answers
`"This API Token is valid and active"`.

### 5. Install lego

```sh
# Git Bash
mkdir -p ~/bin && cd ~/bin
curl -sL -o lego.zip \
  https://github.com/go-acme/lego/releases/download/v5.4.1/lego_v5.4.1_windows_amd64.zip
unzip -o lego.zip lego.exe && rm lego.zip
./lego.exe --version
```

```powershell
# PowerShell
New-Item -ItemType Directory -Force "$HOME\bin" | Out-Null
Set-Location "$HOME\bin"
curl.exe -sL -o lego.zip `
  https://github.com/go-acme/lego/releases/download/v5.4.1/lego_v5.4.1_windows_amd64.zip
Expand-Archive -Path lego.zip -DestinationPath . -Force
Remove-Item lego.zip
.\lego.exe --version
```

`Expand-Archive` extracts the whole archive, unlike `unzip -o lego.zip lego.exe`
which takes one member; the extra files are harmless.

lego **5** takes challenge flags *after* the `run` subcommand. lego 4 took them
before, so older recipes fail with `flag provided but not defined: -dns`.

### 6. Dry run against staging

Staging first, so a mistake does not consume the production limit:

```sh
# Git Bash — run from the repo root
CLOUDFLARE_DNS_API_TOKEN=$(tr -d ' \r\n' < .vscode/cloudflare.token) ~/bin/lego.exe run \
  --dns cloudflare \
  --dns.resolvers 192.168.10.1:53 \
  --dns.propagation.wait 30s \
  --server letsencrypt-staging \
  --domains "*.localbox.ro" --domains localbox.ro \
  --email liviu.voicu@gmail.com --accept-tos \
  --path .vscode/lego > ~/lego-staging.log 2>&1
```

```powershell
# PowerShell — run from the repo root
$env:CLOUDFLARE_DNS_API_TOKEN = (Get-Content .vscode\cloudflare.token -Raw).Trim()
& "$HOME\bin\lego.exe" run `
  --dns cloudflare `
  --dns.resolvers 192.168.10.1:53 `
  --dns.propagation.wait 30s `
  --server letsencrypt-staging `
  --domains "*.localbox.ro" --domains localbox.ro `
  --email liviu.voicu@gmail.com --accept-tos `
  --path .vscode\lego *> "$HOME\lego-staging.log"
```

Wildcards are DNS-01 only — HTTP-01 cannot issue them, which is why the
Cloudflare API token exists at all.

Redirect to a file. Piping through `tail` (or `Select-Object -Last`) buffers the
whole log until exit, so a stall looks identical to normal progress.

Watch it live from a second window:

```sh
# Git Bash
tail -f ~/lego-staging.log
```

```powershell
# PowerShell
Get-Content "$HOME\lego-staging.log" -Wait -Tail 20
```

The staging certificate is issued by `(STAGING) …` and is untrusted by design.
It proves the plumbing, nothing else. Delete it before the real run:

```sh
# Git Bash
rm -rf .vscode/lego/certificates
```

```powershell
# PowerShell
Remove-Item -Recurse -Force .vscode\lego\certificates
```

### 7. Issue the real certificate

```sh
# Git Bash
CLOUDFLARE_DNS_API_TOKEN=$(tr -d ' \r\n' < .vscode/cloudflare.token) ~/bin/lego.exe run \
  --dns cloudflare \
  --dns.resolvers 192.168.10.1:53 \
  --dns.propagation.wait 30s \
  --preferred-chain "ISRG Root X2" \
  --domains "*.localbox.ro" --domains localbox.ro \
  --email liviu.voicu@gmail.com --accept-tos \
  --path .vscode/lego > ~/lego-prod.log 2>&1
```

```powershell
# PowerShell
$env:CLOUDFLARE_DNS_API_TOKEN = (Get-Content .vscode\cloudflare.token -Raw).Trim()
& "$HOME\bin\lego.exe" run `
  --dns cloudflare `
  --dns.resolvers 192.168.10.1:53 `
  --dns.propagation.wait 30s `
  --preferred-chain "ISRG Root X2" `
  --domains "*.localbox.ro" --domains localbox.ro `
  --email liviu.voicu@gmail.com --accept-tos `
  --path .vscode\lego *> "$HOME\lego-prod.log"
```

Takes about 80 seconds. Writes `_.localbox.ro.crt`, `.issuer.crt`, `.key` and
`.json` under `.vscode/lego/certificates/`.

Three flags carry the whole procedure:

- `--dns.resolvers 192.168.10.1:53` — lego cannot read Windows' system
  nameservers and **falls back to `1.1.1.1`**, which this LAN blocks. Without
  this flag one challenge took **5 min 20 s** instead of 0.7 s, logging nothing
  between `preparing to solve the challenge` and the next line.
- `--dns.propagation.wait 30s` — replaces the propagation probe with a fixed
  wait. The probe queries authoritative servers on port 53 directly.
- `--preferred-chain "ISRG Root X2"` — see below. Omit it and the certificate is
  valid but useless to this project.

### 8. Verify against the roots the code actually uses

`openssl verify` with no `-CAfile` checks the OS store, which already trusts
roots `webpki-roots` does not carry. It will say `OK` on a certificate the probe
rejects. Pin the root instead:

```sh
# Git Bash
curl -so isrg-x2.pem https://letsencrypt.org/certs/isrg-root-x2.pem
openssl verify -CAfile isrg-x2.pem \
  -untrusted .vscode/lego/certificates/_.localbox.ro.issuer.crt \
  .vscode/lego/certificates/_.localbox.ro.crt
```

```powershell
# PowerShell
curl.exe -so isrg-x2.pem https://letsencrypt.org/certs/isrg-root-x2.pem
openssl.exe verify -CAfile isrg-x2.pem `
  -untrusted .vscode\lego\certificates\_.localbox.ro.issuer.crt `
  .vscode\lego\certificates\_.localbox.ro.crt
```

`OK` here is the signal that `webpki_roots::TLS_SERVER_ROOTS` will accept it.

Confirm the names too:

```sh
# Git Bash
openssl x509 -in .vscode/lego/certificates/_.localbox.ro.crt \
  -noout -subject -issuer -dates -ext subjectAltName
```

```powershell
# PowerShell
openssl.exe x509 -in .vscode\lego\certificates\_.localbox.ro.crt `
  -noout -subject -issuer -dates -ext subjectAltName
```

Wanted: `DNS:*.localbox.ro, DNS:localbox.ro`, and an issuer of `YE2` (or another
`YE`/`YR` intermediate) whose own chain reaches `ISRG Root X2`.

## The chain trap

Let's Encrypt moved to the **Generation Y** hierarchy on 2026-01-07. The default
chain now ends at `ISRG Root YE`, which no released `webpki-roots` carries —
checked in 0.26.11, 1.0.8 (the version `fah-http` resolves) and 1.0.9. A
default-chain certificate is genuine, publicly trusted, accepted by every
browser, and **rejected by this project** with `UnknownIssuer`.

The new roots are cross-signed from the old ones, so a compatible chain is
always offered as an ACME *alternate*. Which old root depends on the leaf's key
type:

| Leaf key | Intermediate | Cross-signed chain ends at |
| -------- | ------------ | -------------------------- |
| ECDSA (lego's `EC256` default) | `YE1` / `YE2` / `YE3` | `ISRG Root X2` |
| RSA | `YR1` / `YR2` / `YR3` | `ISRG Root X1` |

`--preferred-chain "ISRG Root X1"` on an ECDSA leaf matches nothing, so lego
keeps the default silently — no warning, no error, and a certificate that fails
later. Always verify the output rather than trusting the flag.

Full analysis, including how to read the alternates without spending issuance
quota:
[`docs/solutions/environment/lets-encrypt-gen-y-chain-vs-webpki-roots.md`](solutions/environment/lets-encrypt-gen-y-chain-vs-webpki-roots.md).

## Renewing

Due **after 2026-11-08**; the certificate dies 2026-12-08. lego refuses early
with `Skip renewal: The certificate expires at …`, which is not an error.

That date is not something to track by hand. lego 5 has no separate `renew`
subcommand — `run` is "get or renew" and decides for itself, defaulting to
one third of the certificate's lifetime remaining, which is 30 days on a 90-day
certificate and lands exactly on 2026-11-08. `--renew-days N` overrides it.
Running `run` on a schedule is therefore safe: outside the window it is a no-op
that costs one ACME directory fetch. Run it weekly rather than monthly - see
§Setting up the scheduled tasks for why the cadence changes the margin.

One flag matters for automation. lego adds a **random sleep before a renewal**
and its own help recommends against `--no-random-sleep` for automated runs, so a
real renewal takes considerably longer than the command suggests. That needs no
action — a `schtasks`-created task allows `PT72H`, measured — but it does mean a
renewal run is not something to watch and cancel when it seems to hang.

### Before you run anything: the token file is empty

`.vscode/cloudflare.token` is **0 bytes** (checked 2026-09-17). Every command
below reads it, and an empty token produces a Cloudflare `401` part-way through
the DNS-01 challenge — after lego has already opened an ACME order, which spends
one of the five failed validations per hour.

Re-create it first, per §4, and check it is non-empty before starting:

```powershell
(Get-Item .vscode\cloudflare.token).Length   # must not be 0
```

The token needs `Zone:DNS:Edit` on `localbox.ro` and nothing else. Keeping the
file empty between renewals is reasonable hygiene — it just has to be filled
before, not during.

### The command

Same command as step 7 — the flags are not optional:

```sh
# Git Bash
CLOUDFLARE_DNS_API_TOKEN=$(tr -d ' \r\n' < .vscode/cloudflare.token) ~/bin/lego.exe run \
  --dns cloudflare \
  --dns.resolvers 192.168.10.1:53 \
  --dns.propagation.wait 30s \
  --preferred-chain "ISRG Root X2" \
  --domains "*.localbox.ro" --domains localbox.ro \
  --email liviu.voicu@gmail.com --accept-tos \
  --path .vscode/lego
```

```powershell
# PowerShell
$env:CLOUDFLARE_DNS_API_TOKEN = (Get-Content .vscode\cloudflare.token -Raw).Trim()
& "$HOME\bin\lego.exe" run `
  --dns cloudflare `
  --dns.resolvers 192.168.10.1:53 `
  --dns.propagation.wait 30s `
  --preferred-chain "ISRG Root X2" `
  --domains "*.localbox.ro" --domains localbox.ro `
  --email liviu.voicu@gmail.com --accept-tos `
  --path .vscode\lego
```

Then re-run the step 8 verification. A renewal that drops `--preferred-chain`
produces a working-looking certificate that the probe refuses.

**`--preferred-chain` fails silently.** lego's own help: *"If no match, the
default offered chain will be used."* A wrong or misspelled chain name is not an
error — it is a certificate that verifies against the OS store, serves cleanly
in a browser, and is rejected by `webpki-roots`. Nothing tells you until the
probe does. Step 8 is the only thing standing between that and a deploy, so it
is not optional either.

To re-issue before the renewal window — to change the chain, say — delete
`.vscode/lego/certificates` and run again. `--renew-force` does the same thing.
Mind the limit below before doing this repeatedly.

### Renewing is not finished until the router has it

A new file in `.vscode/lego/certificates/` changes nothing the household sees.
The pair has to reach the container's `/config`, and **the container has to
restart**:

```powershell
scp .vscode\lego\certificates\_.localbox.ro.crt `
    bobdenaut:kingston/fastadhunter/config/api-cert.pem
scp .vscode\lego\certificates\_.localbox.ro.key `
    bobdenaut:kingston/fastadhunter/config/api-key.pem

ssh bobdenaut '/container/stop [find name~"fastadhunter"]'
ssh bobdenaut '/container/start [find name~"fastadhunter"]'
```

The two `ssh` lines work as written from PowerShell 7.3+ and from Bash. From
Windows PowerShell 5.1 the inner quotes must be escaped —
`[find name~\"fastadhunter\"]` — because 5.1 copies the argument into ssh's
command line unescaped, ssh's parser eats the quotes, and the router sees a bare
word that matches no container. Both commands then do nothing, print nothing and
exit 0. Verified on the router 2026-09-19: the bare form returned no id, the
quoted form returned `*2B`. `scripts/renew-certificate.ps1 -Deploy` handles both
shells and waits for `running=false` before starting.

The restart is **not optional and cannot be avoided by the API**. `ApiServer::
bind` takes an `Option<Arc<rustls::ServerConfig>>` and builds its `TlsAcceptor`
from it once (`fah-api/src/server.rs:59-74`); there is no `ResolvesServerCert`
and no swap cell, so the loaded pair is fixed for the life of the process. That
applies to `POST /api/v1/certificates/import` too — the route rewrites the files
on disk, but the listener keeps serving the old certificate until the container
comes back.

Budget a short DNS outage for the restart: this container is the household's
only resolver. From Phase 3 onward the same restart also reloads DoT, which
serves this certificate.

### Automating it

`scripts/renew-certificate.ps1` wraps everything above for Task Scheduler. It
is ASCII-only and written for PowerShell 5.1. It derives every path from its own
location, so it runs correctly from any working directory — which matters,
because Task Scheduler does not set one. It re-reads PATH from the registry on
start, since a scheduler session can predate the OpenSSL install.

#### Parameters

| Parameter | Effect |
| --------- | ------ |
| *(none)* | Preflight, then lego, then verify. Leaves the result on the dev box and touches nothing remote. |
| `-CheckOnly` | Report only. Reads the certificate off the **live listener**, logs the days remaining, exits `2` when under the threshold. Runs nothing else. |
| `-WarnDays <n>` | Threshold for `-CheckOnly`. Default `30`. Ignored in every other mode. |
| `-Deploy` | After a successful renewal: `scp` the pair to `/config`, restart the container, then confirm the listener serves the new date. |
| `-Force` | Adds `--renew-force`, re-issuing even when not due. Mind the 5-duplicates-per-week limit. Does **not** deploy on its own — combine with `-Deploy`. |

Two rules about how they combine:

- **`-CheckOnly` wins over everything.** It returns before the renewal logic is
  reached, so `-CheckOnly -Deploy` reports and exits `0`; it does not deploy.
- **`-Force` is not a deploy.** `-Force` alone re-issues and stops on the dev
  box. Reaching the router always requires `-Deploy`.

#### What a full run does, in order

1. Re-reads PATH; fails if `openssl` is still missing.
2. Checks `lego.exe`, the certificate store, and that the token file is
   **non-empty** — this stops before lego rather than burning a failed
   validation on a `401`.
3. Records the current certificate's fingerprint and expiry.
4. Runs lego with the flags from §The command, redirecting (not piping) to a
   per-run log. Clears `CLOUDFLARE_DNS_API_TOKEN` from the environment
   afterwards, even on failure.
5. Compares the fingerprint. Unchanged means not due: logs it and exits `0`
   without deploying.
6. **Runs step 8 in code** — downloads the ISRG Root X2 root and does
   `openssl verify -CAfile <root> -untrusted <issuer> <cert>`. A cryptographic
   path validation, not a text match: the string `ISRG Root X2` also appears in
   the wrong chain, so a substring test would accept both. Anything other than
   `OK` aborts before the router is touched.
7. With `-Deploy` only: copies the pair, restarts the container, then polls the
   listener for up to 120 s and checks it now serves the new expiry date.

Because of step 6 a renewal run needs to reach `letsencrypt.org` as well as the
ACME and Cloudflare endpoints.

#### Exit codes

| Code | Meaning |
| ---- | ------- |
| `0` | Success, or nothing was due |
| `2` | `-CheckOnly` only: fewer than `-WarnDays` days remain |
| `1` | Any failure — empty token, lego error, chain not verified, scp or restart failed |

Task Scheduler shows these in its "Last Run Result" column, so a bad run is
visible without opening a log.

#### Two failure modes worth knowing

- **`-CheckOnly` reads the live listener, not the local file.** If the container
  is down it exits `1` with "is the container up?". That makes the daily task a
  liveness check as well as an expiry check — but it also means exit `1` there
  usually means "FastAdHunter is not answering", not "renewal is broken".
- **A half-copied pair.** The certificate and the key are two separate `scp`
  calls. If the first succeeds and the second fails, `/config` holds a
  mismatched pair and the script stops *before* restarting — so the running
  container is unaffected and still serving the old certificate. The log says
  so explicitly. Fix the pair before restarting anything.

#### Setting up the scheduled tasks

**The default run never touches the router.** Deployment is opt-in precisely
because it restarts the household's resolver.

Two tasks are enough — daily warning, weekly renewal. Everything below was
created, run and deleted on this box on 2026-09-17, so the defaults quoted are
measured rather than assumed.

```powershell
$ps = 'powershell.exe'
$sc = 'E:\FastAdHunter\scripts\renew-certificate.ps1'

# Daily 08:00 - warn only, exit code 2 when under 30 days
schtasks /Create /TN "FAH cert check" /SC DAILY /ST 08:00 /F `
  /TR "$ps -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$sc`" -CheckOnly"

# Weekly, Sunday at 04:00 - renew and deploy if due; a no-op otherwise
schtasks /Create /TN "FAH cert renew" /SC WEEKLY /D SUN /ST 04:00 /F `
  /TR "$ps -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$sc`" -Deploy"
```

Any cadence is safe because lego decides: outside the window the run costs one
ACME directory fetch and exits having changed nothing.

**Weekly, not monthly, and the reason is arithmetic rather than taste.** The
renewal window opens 30 days before expiry, which for the current pair is
2026-11-08. A task running on the 1st of each month meets that as follows:

| Run | Outcome | Margin if it renews |
| --- | ------- | ------------------- |
| 2026-11-01 | skips — 7 days too early | — |
| 2026-12-01 | renews | **7 days** |
| *weekly, 2026-11-09* | renews | **29 days** |

Monthly leaves a week to absorb an empty token, a Cloudflare hiccup or a
rate-limit lockout, and the next attempt after a failure is a month away. Weekly
costs four directory fetches a month and leaves four times the runway. The
`-CheckOnly` task is an independent nudge either way: it starts returning `2`
the day the window opens.

04:00 is for the **restart**, not for lego. The renewal itself is
time-of-day-agnostic; what wants an unsociable hour is the container coming back,
because it takes the household off DNS while it does.

`-WindowStyle Hidden` keeps a console from flashing up every morning. Neither
task needs elevation — the script writes only inside the repo and talks over the
network.

#### Three schtasks defaults that are wrong for this job

Measured on the tasks created above. Fix them or know you have accepted them.

| Setting | schtasks default | Why it matters |
| ------- | ---------------- | -------------- |
| `LogonType` | **`Interactive`** — "Interactive only" | The task does not run unless that user is logged on. A laptop sitting at the lock screen is fine; one that is logged out is not. |
| `StartWhenAvailable` | **`False`** | A missed run is **not** made up. If the machine is off at 04:00 on a Sunday, that week's renewal attempt simply never happens — silently. Weekly means the next attempt is seven days away rather than thirty, which softens this, but it does not remove it. This is the one that actually loses you a certificate. |
| `ExecutionTimeLimit` | `PT72H` (3 days) | Already ample. lego's random pre-renewal sleep is nowhere near this, so nothing needs changing — noted only because it is the setting people reach for first. |

`schtasks` cannot set `StartWhenAvailable`; use the scheduler cmdlets:

```powershell
foreach ($n in 'FAH cert check','FAH cert renew') {
  $s = Get-ScheduledTask -TaskName $n
  $s.Settings.StartWhenAvailable = $true
  Set-ScheduledTask -TaskName $n -Settings $s.Settings | Out-Null
}
```

To run when logged off as well, recreate with stored credentials —
`schtasks /Create … /RU liviu /RP *` prompts for the password and switches the
task to `Password` logon type. Storing the password is a real trade; interactive
only is a defensible choice on a personal dev box, as long as you know a
logged-out month is a skipped month.

One more thing the defaults get right by accident: `Start In` is `N/A`, meaning
the task has no working directory. The script does not care — it derives every
path from its own location — but anything else you schedule here will.

#### Dry-testing a scheduled task

Do this once after creating them. It takes a minute and catches quoting
mistakes that would otherwise surface in November.

**1. Check the command line the task actually stored.** `schtasks` strips the
outer quotes, so this is where a path problem shows up:

```powershell
schtasks /Query /TN "FAH cert check" /V /FO LIST |
  Select-String 'Task To Run|Run As User|Logon Mode|Start In'
```

Expected — note the script path is stored *unquoted*, which is fine here only
because the path has no spaces:

```text
Task To Run:  powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File E:\FastAdHunter\scripts\renew-certificate.ps1 -CheckOnly
Run As User:  liviu
Logon Mode:   Interactive only
Start In:     N/A
```

**2. Fire it by hand** rather than waiting for 08:00:

```powershell
schtasks /Run /TN "FAH cert check"
Start-Sleep -Seconds 15
schtasks /Query /TN "FAH cert check" /V /FO LIST | Select-String 'Last Result'
```

**3. Read the result code.** These are the three you will see:

| `Last Result` | Meaning |
| ------------- | ------- |
| `0` | ran, certificate has more than `-WarnDays` left |
| `2` | ran, renewal is due — this is the warning firing |
| `1` | the script failed; read the log |
| `267011` | **never run.** `0x41303`, not an error — it is what a freshly created task reports |
| `267009` | still running; wait and query again |

`267011` on a new task is the one that looks alarming and is not.

**4. Confirm the script actually logged it**, so you know the result came from
the script and not from the scheduler failing to launch it:

```powershell
Get-Content ".vscode\lego\logs\renew-$(Get-Date -f yyyy-MM-dd).log" -Tail 3
```

**5. Prove the warning path works**, since a daily task that can only ever
report `0` tells you nothing. Force it by raising the threshold above the days
remaining — on a fresh 90-day certificate, `-WarnDays 200` always trips:

```powershell
schtasks /Create /TN "FAH cert DRYTEST" /SC DAILY /ST 08:00 /F `
  /TR "$ps -NoProfile -ExecutionPolicy Bypass -File `"$sc`" -CheckOnly -WarnDays 200"
schtasks /Run /TN "FAH cert DRYTEST"
Start-Sleep -Seconds 15
schtasks /Query /TN "FAH cert DRYTEST" /V /FO LIST | Select-String 'Last Result'
schtasks /Delete /TN "FAH cert DRYTEST" /F
```

`Last Result: 2` is the pass. Delete the throwaway task afterwards — the last
line does it.

There is no dry test for the renewal task short of a real renewal: `-Deploy`
cannot be rehearsed without issuing a certificate and restarting the container.
What the steps above do establish is that the scheduler can launch the script,
that its exit codes arrive intact, and that it writes a log — which is every
part of the chain except lego itself.

#### Logs

`.vscode/lego/logs/` (gitignored): one `renew-YYYY-MM-DD.log` per day, plus a
`lego-YYYY-MM-DD-HHmmss.log` holding the raw lego output of each renewal
attempt.

### Rate limits worth knowing

| Limit | Value |
| ----- | ----- |
| Certificates per registered domain | 50 / week |
| **Duplicate certificates** (identical SAN set) | **5 / week** |
| Failed validations | 5 / hour |

The duplicate limit is the one that bites while chasing a chain. Read the ACME
alternates instead of re-issuing — it costs nothing:

```sh
# Git Bash
# lego pretty-prints the JSON, so the pattern has to allow the space after the
# colon; without it the match is empty and curl fails on a blank argument.
curl -sI "$(grep -o '"certUrl": *"[^"]*"' \
  .vscode/lego/certificates/_.localbox.ro.json | cut -d'"' -f4)" | grep -i '^link'
```

```powershell
# PowerShell
$certUrl = (Get-Content .vscode\lego\certificates\_.localbox.ro.json |
            ConvertFrom-Json).certUrl
curl.exe -sI $certUrl | Select-String '^link'
```

Each `rel="alternate"` link returns a full PEM chain to inspect.

## Troubleshooting

| Symptom | Cause | Fix |
| ------- | ----- | --- |
| `flag provided but not defined: -dns` | lego 5 moved challenge flags after `run` | put them after `run` |
| Hangs at `preparing to solve the challenge`, ~5 min per name | lego fell back to `1.1.1.1`; LAN blocks port 53 | `--dns.resolvers 192.168.10.1:53` |
| No log output at all while running | output piped instead of redirected; the pipe buffers to exit | `> file 2>&1` in Bash, `*> file` in PowerShell |
| Second run behaves oddly, TXT records vanish | an interrupted lego is still running | kill the orphan before retrying — see below |
| `Skip renewal: The certificate expires at …` | not yet in the renewal window | `--renew-force`, or delete `certificates/` |
| `Status: 2`, `lame delegation` on the NS query | registry still holds the old nameservers | wait; it cleared in under 60 s here |
| `UnknownIssuer` from the probe, but `openssl verify` says OK | default Gen Y chain; OS store trusts it, `webpki-roots` does not | `--preferred-chain "ISRG Root X2"` |
| ROTLD WHOIS stops answering | rate-limited by a name sweep | wait, or check on the registrar's page |
| `Invoke-WebRequest : A parameter cannot be found that matches '-s'` | typed `curl` in PowerShell, which is an alias | type `curl.exe` |
| Cloudflare rejects a valid-looking token | trailing newline from `Get-Content -Raw` | `.Trim()` |

Finding and killing an orphaned lego:

```sh
# Git Bash
ps -W | grep -i lego
kill -9 <pid>
```

```powershell
# PowerShell
Get-Process lego -ErrorAction SilentlyContinue
Stop-Process -Name lego -Force
```

## Open items

- Three records exist, all verified 2026-09-17: `router.localbox.ro` →
  `192.168.10.1` (RouterOS WebFig on 8443), `fah-api.localbox.ro` →
  `172.17.0.2` (the FastAdHunter API) and `fah-dot.localbox.ro` → `172.17.0.2`
  (the DoT name decided on 2026-09-13, created the same day). Port 853 still
  answers nowhere — 0.3.4 has no DoT listener — so a probe gets a refused
  connection, not a certificate error, until 0.4.0 is deployed. The deployment
  is **LAN-only** — see the decision below.
- **Settled 2026-09-17: `fah-dot.localbox.ro` is the DoT name.** Both records
  point at `172.17.0.2` and either would work, so this is a naming decision
  rather than a functional one — keeping them apart means the DoT record can be
  repointed later without moving the dashboard. `docs/0.4.0-install.md` §7 and
  its §9 verification commands were changed to match; `fah-api.localbox.ro`
  stays the name for the API, the dashboard and DoH.
- **Deployed on the API since 2026-09-09.** The pair was copied onto the
  container's `/config` volume as `api-cert.pem` / `api-key.pem` and picked up
  at the restart of `14:15:15` local. `https://fah-api.localbox.ro:8443` now
  verifies strictly — `curl` without `-k` answers `200`. The replaced
  self-signed pair, and the rest of `/config`, is backed up outside the repo.
  The `POST /api/v1/certificates/import` route was **not** used: it does not
  exist in 0.3.3, which predates Phase 3. 0.4.0 has it, but it does not remove
  the restart — see §Renewing. Replacing the files on disk stays the simplest
  route either way.
- Two names lost their clean padlock in the swap: `172.17.0.2` and
  `fastadhunter` were SANs of the old self-signed certificate and are not on
  this one, which covers `*.localbox.ro` and `localbox.ro` only. Reach the API
  by name.
- Renewal is manual in the sense that nothing in FastAdHunter does it, and
  nothing will: an ACME client in the binary would need a Cloudflare
  zone-edit token stored on the router, which is a worse trade than a
  quarterly task. `scripts/renew-certificate.ps1` automates the dev-box side
  and is meant to be driven by Task Scheduler; 2026-11-08 otherwise exists only
  in this document and in the Let's Encrypt expiry email.
- The private key lives in `.vscode/lego/` on the dev box, gitignored and
  unbacked-up. Losing it means re-issuing, which is cheap — but so is copying it
  somewhere safe.
- **`_.localbox.ro.json` records `"preferredChain": "ISRG Root X1"`, which is
  not what this document tells you to pass.** The files on disk are right: the
  delivered `.crt` runs leaf ← `YE2` ← `Root YE` ← `ISRG Root X2` and verifies
  against the X2 root, while `.crt.default-chain` holds the longer alternate
  continuing `ISRG Root X2` ← `ISRG Root X1`. So the deployed certificate is
  correct and the metadata disagrees with it. Unresolved — found by dry-running
  these commands on 2026-09-17, not by any failure. Since `--preferred-chain`
  falls back silently, do not assume the next renewal reproduces this chain:
  run step 8 and read the result.
- Step 8 writes `isrg-x2.pem` into whatever directory it runs from, and `*.pem`
  is **not** gitignored at the repo root. Run it from a scratch directory or
  delete the file afterwards, or it shows up as untracked.

## Why the deployment is LAN-only, 2026-09-09

Settled after working through every option. Recorded so it is not re-opened from
scratch — the answer turns on one fact about the mobile carrier, not on taste.

**The requirement.** Filter the household phone on mobile data, while letting
only that phone reach FastAdHunter. Nothing else on the internet.

**Why that is hard.** Android's Private DNS presents **no credential of any
kind** — the setting is a hostname field. It authenticates the server and
identifies itself to nobody. So the only thing that arrives at the firewall is a
source address, and any "allow only my phone" rule has to be an address rule.

| Option | Verdict |
| ------ | ------- |
| WireGuard tunnel | rejected — interferes with Android Auto |
| Client certificate (mTLS) | **impossible.** Private DNS cannot present one whoever signs it. `fah-certs` could mint one tomorrow; there is no client to use it. `crates/fah-dns/src/dot.rs:40` is `.with_no_client_auth()`, but changing that would only lock the phone out too |
| Allowlist the carrier's ranges | rejected — admits every Orange RO mobile subscriber |
| Secret path on DoH | declined. Would also need a new listener: DoH is a route on the API listener (`https://<api>/dns-query`), so forwarding its port publishes the dashboard and API too |
| Allowlist the phone's IPv6, kept current by DDNS | **dead — no IPv6 on the phone** |
| Allowlist the phone's IPv4 | pointless — Orange mobile is CGNAT, so the address is shared with everyone behind that NAT |

**The IPv6 finding, which is the decisive one.** The home side is ready: DIGI
provides a routable prefix, and the router already tracks it dynamically —
`fah-lan6` holds `2a02:2f04:540c:9800::/56`. With no NAT on IPv6 the phone would
have had its own globally-routable address, and an address-list entry would have
meant exactly one device.

Orange does not provide it. `test-ipv6.com` on mobile data reports **no IPv6**
with the APN protocol already set to `IPv4/IPv6`, so this is not a handset
misconfiguration. Every remaining approach collapses to IPv4, where CGNAT means
the phone's address is not the phone's.

**Consequence.** FastAdHunter is reachable on the LAN only. The phone is
filtered on home Wi-Fi and unfiltered on mobile data, by design. For mobile, a
hosted filtering resolver such as NextDNS works with the stock Private DNS
setting and publishes nothing of ours.

**What would re-open this.** Orange provisioning IPv6 on mobile. Nothing else
changes the analysis — the home side is already prepared, and the IPv6 allowlist
becomes buildable the day the phone gets an address.

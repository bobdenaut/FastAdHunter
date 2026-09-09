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
| ACME client | `lego 5.4.1` at `~/bin/lego.exe` |

## Shells

The host is **Windows 11**. Every command below is given twice — Git Bash first
(what was actually run), then the PowerShell equivalent. Use either; do not mix
them inside one step.

Windows PowerShell **5.1** is what ships here (`$PSVersionTable.PSVersion`), not
PowerShell 7. The equivalents are written for 5.1 and use nothing newer.

Four differences that produce wrong results rather than errors:

- **`curl` is not curl in PowerShell.** It is an alias for `Invoke-WebRequest`,
  which does not understand `-s`, `--resolve` or `-I`. Always type `curl.exe`.
  On this box that resolves to `C:\Program Files\Git\mingw64\bin\curl.exe`,
  the same binary Git Bash uses.
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

`openssl.exe`, `whois.exe` and `nslookup.exe` are on PATH in both shells —
openssl and curl come from the Git for Windows install, whois from Sysinternals.

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
| Name | `dot` |
| IPv4 | `192.168.10.1` |
| Proxy status | **DNS only** (grey cloud) |

Proxying must be off. It replaces the answer with Cloudflare's addresses and
covers HTTP ports only, which would break DoT on 853 and point clients at
Cloudflare instead of the box. For a private address Cloudflare forces DNS only
by itself and labels the record `DNS only - reserved`.

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

To re-issue before the renewal window — to change the chain, say — delete
`.vscode/lego/certificates` and run again. `--renew-force` does the same thing.
Mind the limit below before doing this repeatedly.

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
curl -sI "$(grep -o '"certUrl":"[^"]*"' \
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

- `dot.localbox.ro` resolves to `192.168.10.1`. The deployment is **LAN-only** —
  see the decision below. The address a listener actually answers on still
  depends on Runbook 1, which has not run.
- Nothing serves this certificate yet. It is verified as a file on the dev box.
- Renewal is manual. There is no timer, no hook and no CI; 2026-11-08 exists
  only in this document and in the Let's Encrypt expiry email.
- The private key lives in `.vscode/lego/` on the dev box, gitignored and
  unbacked-up. Losing it means re-issuing, which is cheap — but so is copying it
  somewhere safe.

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

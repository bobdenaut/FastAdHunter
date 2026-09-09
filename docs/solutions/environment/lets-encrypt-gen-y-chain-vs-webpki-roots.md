---
title: A Let's Encrypt certificate that webpki-roots refuses, and the cross-signed chain that fixes it
date: 2026-09-09
category: environment
module: fah-http
problem_type: environment_trap
component: infrastructure
severity: high
applies_when:
  - a component verifies an upstream against webpki_roots::TLS_SERVER_ROOTS rather than the OS trust store
  - a certificate was obtained from Let's Encrypt on or after 2026-01-07
  - an ACME client ran with its default chain selection
  - openssl or a browser accepts the certificate but the Rust code rejects it
symptoms:
  - rustls fails the upstream handshake with UnknownIssuer while `openssl verify` on the same files answers OK
  - the leaf's issuer reads `CN=YE1`, `CN=YE2`, `CN=YE3`, `CN=YR1`, `CN=YR2` or `CN=YR3`
  - the chain's topmost certificate is issued by `C=US, O=ISRG, CN=Root YE` or `CN=Root YR`
---

## The trap

Let's Encrypt moved to the **Generation Y** hierarchy on **2026-01-07**. New
issuance chains to `ISRG Root YE` (ECDSA) or `ISRG Root YR` (RSA) instead of
`ISRG Root X1` / `ISRG Root X2`.

`webpki-roots` does not carry the Y roots. Checked against the two versions in
the lockfile:

| Version | ISRG roots present |
| ------- | ------------------ |
| 0.26.11 | `ISRG Root X1`, `ISRG Root X2` |
| 1.0.8 (used by `fah-http`) | `ISRG Root X1`, `ISRG Root X2` |
| 1.0.9 | `ISRG Root X1`, `ISRG Root X2` |

`crates/fah-http/src/tls.rs:38` builds its root store from
`webpki_roots::TLS_SERVER_ROOTS`, so a default-chain Let's Encrypt certificate
issued today is rejected with `UnknownIssuer` — **even though it is a genuine,
publicly trusted certificate**. Bumping the dependency does not help; no
published version carries the Y roots yet.

The failure is easy to misread because the OS trust store already has the Y
roots. `openssl verify`, `curl` and every browser accept the certificate. Only
the Rust path refuses it.

## Why a fix exists

Let's Encrypt **cross-signed** the new roots from the old ones — `Root YE` from
`ISRG Root X2`, `Root YR` from `ISRG Root X1`. The ACME API offers the
cross-signed chain as an *alternate*, so a compatible chain is always available;
it just is not the default.

Which old root a certificate lands under follows the leaf's key type:

| Leaf key | Intermediate | Cross-signed chain terminates at |
| -------- | ------------ | -------------------------------- |
| ECDSA (lego's `EC256` default) | `YE1` / `YE2` / `YE3` | `ISRG Root X2` |
| RSA | `YR1` / `YR2` / `YR3` | `ISRG Root X1` |

## Finding the right chain without spending rate limit

Let's Encrypt caps **duplicate certificates at 5 per week** for one SAN set, so
re-issuing to find the right chain is expensive. Reading the alternates costs
nothing — they hang off the certificate URL the ACME client already stored:

```sh
# Git Bash
curl -sI "$(grep -o '"certUrl":"[^"]*"' cert.json | cut -d'"' -f4)" | grep -i '^link'
```

```powershell
# PowerShell
$certUrl = (Get-Content cert.json | ConvertFrom-Json).certUrl
curl.exe -sI $certUrl | Select-String '^link'
```

Each `rel="alternate"` link returns a full PEM chain. Split it and read the last
certificate's issuer to see which root it terminates at:

```sh
# Git Bash
curl -s "<alternate-url>" -o alt.pem
openssl crl2pkcs7 -nocrl -certfile alt.pem | openssl pkcs7 -print_certs -noout
```

```powershell
# PowerShell
curl.exe -s "<alternate-url>" -o alt.pem
openssl.exe crl2pkcs7 -nocrl -certfile alt.pem |
  openssl.exe pkcs7 -print_certs -noout
```

## Requesting it

For an ECDSA leaf, name the root the cross-sign lands on — not the root that
signed the intermediate:

```sh
# Git Bash
~/bin/lego.exe run --dns cloudflare \
  --preferred-chain "ISRG Root X2" \
  --domains "*.localbox.ro" --domains localbox.ro \
  --email liviu.voicu@gmail.com --accept-tos --path .vscode/lego
```

```powershell
# PowerShell
& "$HOME\bin\lego.exe" run --dns cloudflare `
  --preferred-chain "ISRG Root X2" `
  --domains "*.localbox.ro" --domains localbox.ro `
  --email liviu.voicu@gmail.com --accept-tos --path .vscode\lego
```

`--preferred-chain "ISRG Root X1"` silently does nothing for an ECDSA leaf: no
offered chain matches, so lego keeps the default and writes a chain that still
ends at `Root YE`. There is no warning. Verify the output rather than trusting
the flag.

## Verifying against the same roots the code uses

`openssl verify` with the OS store proves nothing here. Pin the root explicitly
to the one `webpki-roots` actually carries:

```sh
# Git Bash
curl -so isrg-x2.pem https://letsencrypt.org/certs/isrg-root-x2.pem
openssl verify -CAfile isrg-x2.pem -untrusted chain.pem leaf.pem
```

```powershell
# PowerShell
curl.exe -so isrg-x2.pem https://letsencrypt.org/certs/isrg-root-x2.pem
openssl.exe verify -CAfile isrg-x2.pem -untrusted chain.pem leaf.pem
```

`OK` from that command is the signal that `TLS_SERVER_ROOTS` will accept it.

## A second trap on the same path

`lego` reads system nameservers to resolve zones and check propagation. On
Windows it cannot determine them and **falls back to `1.1.1.1`** — as its own
`--dns.resolvers` help states. On a network that blocks outbound port 53, every
lookup burns a timeout chain: one challenge took **5 min 20 s** instead of under
a second, and lego logs nothing between `preparing to solve the challenge` and
the next line, so it looks like a hang rather than a DNS failure.

Point it at a reachable resolver:

```sh
--dns.resolvers 192.168.10.1:53
```

Same run afterwards: 0.7 s per challenge.

Two collection notes that cost time here — redirect lego to a file, never pipe
it, or the whole log stays buffered until exit and a stall is invisible; and an
interrupted lego keeps running, so check for an orphan before starting again.
Two concurrent runs on one domain clean up each other's `_acme-challenge`
records.

```sh
# Git Bash — redirect, watch, find an orphan
… --path .vscode/lego > ~/lego.log 2>&1
tail -f ~/lego.log
ps -W | grep -i lego
```

```powershell
# PowerShell — same three
… --path .vscode\lego *> "$HOME\lego.log"
Get-Content "$HOME\lego.log" -Wait -Tail 20
Get-Process lego -ErrorAction SilentlyContinue
```

## Shell note

The host is Windows 11 with Windows PowerShell 5.1. In PowerShell, `curl` is an
alias for `Invoke-WebRequest` and does not accept `-s`, `-I` or `--resolve` —
always type `curl.exe`. Both shells resolve `curl.exe` and `openssl.exe` to the
Git for Windows binaries, so behaviour is identical; note that curl there is
Schannel-backed and ignores `--cacert`, which is why the verification above uses
`openssl` instead.

# Nexus Domains & Hosting

## Purpose

`Domains & Hosting` is the Nexus operational control plane for DreamtecLabs domains that use Cloudflare DNS/Tunnel, the central residential-IP DDNS service, Hestia mail/webmail, and the DreamtecLabs SMTP relay.

The first implementation deliberately keeps provider credentials server-side and separates read-only validation from privileged onboarding. It does not expose Cloudflare tokens, SSH keys, Hestia credentials, mailbox passwords, or SMTP credentials to the browser.

## Ownership model

Nexus is the source of truth for intended hostname ownership and policy. Provider systems remain the execution targets.

| Hostname/capability | Owner | Required behavior |
| --- | --- | --- |
| `mail.<domain>` | Central DDNS + Cloudflare DNS | A record, DNS only, dynamic residential public IP |
| `smtp.*` | Cloudflare DNS | DNS only; never Cloudflare proxied |
| `webmail.<domain>` | Cloudflare Tunnel | Tunnel-managed; never present in central DDNS |
| MX | Cloudflare DNS | Points directly to `mail.<domain>`; never to a CNAME |
| SPF | Cloudflare DNS | Exactly one SPF record; includes current residential IP and SMTP relay IP |
| DKIM | Hestia -> Cloudflare DNS | Public key comes from Hestia |
| DMARC | Cloudflare DNS | Starts at `p=none` unless explicitly configured otherwise |
| Hestia mail domain | Hestia | Exim/Dovecot/Roundcube mail domain and DKIM |
| Webmail certificate | Hestia + Tunnel | Initial HTTP origin for issuance, then HTTPS origin with `No TLS Verify` |

The helper refuses onboarding if `webmail.<domain>` is still present in `/etc/cloudflare-ddns/records.conf`. This is intentional: Nexus will not silently create dual ownership between DDNS and Tunnel.

## Current bootstrap inventory

When `/etc/proxmox-datacenter-manager/domains-hosting.json` does not yet exist, Nexus exposes a bootstrap inventory based on the currently known environment:

- `dreamteclabs.com`
- `kinpilot.app`
- `savipilot.com`
- `domuspilot.com`
- `mundoleo.co`
- `dreamtec.com.br`
- `claudiokaist.com`

The bootstrap inventory is only an initial state. Create `/etc/proxmox-datacenter-manager/domains-hosting.json` to make inventory changes explicit and versionable through operations/change management.

## API and privileges

The backend registers `/api2/json/domains` through the existing PDM/Nexus API router.

- `GET /domains`: requires `PRIV_SYS_AUDIT`; returns inventory and policy only.
- `POST /domains/validate`: requires `PRIV_SYS_AUDIT`; performs read-only live checks.
- `POST /domains/onboard`: requires `PRIV_SYS_MODIFY`; invokes the privileged helper and then performs final validation.

Validation covers:

- public A resolution for `mail.<domain>` and `webmail.<domain>`;
- MX;
- exactly one SPF record;
- DKIM presence;
- exactly one DMARC record;
- SMTP submission STARTTLS on port 587 with certificate verification;
- IMAPS on port 993 with certificate verification;
- webmail HTTPS on port 443 with certificate verification.

Results are shown in the Nexus UI. Validation and onboarding outcomes are appended to `/etc/proxmox-datacenter-manager/domains-hosting-audit.log` without secrets.

## Server prerequisites

The Nexus server needs:

- `bash`
- `curl`
- `jq`
- `dig` (`dnsutils`/`bind9-dnsutils`, depending on the base image/package set)
- `openssl`
- `ssh`
- key-based, non-interactive SSH access to the Hestia host and the Cloudflare/DDNS LXC

The installed helper path is:

`/usr/libexec/proxmox/nexus-domains-helper`

The helper reads runtime integration settings from:

`/etc/proxmox-datacenter-manager/domains-hosting.env`

The environment file must be owned by root and mode `0600`.

Example, with placeholders for secrets/installation-specific identifiers:

```sh
NEXUS_HESTIA_SSH_HOST=root@192.168.0.29
NEXUS_CLOUDFLARE_SSH_HOST=root@<cloudflare-lxc-host-or-ip>
NEXUS_CF_API_TOKEN=<cloudflare-api-token>
NEXUS_CF_ACCOUNT_ID=<cloudflare-account-id>
NEXUS_CF_TUNNEL_ID=<cloudflare-tunnel-id>

NEXUS_DDNS_RECORDS_FILE=/etc/cloudflare-ddns/records.conf
NEXUS_DDNS_SERVICE=cloudflare-ddns.service
NEXUS_HESTIA_BIN=/usr/local/hestia/bin
NEXUS_MAIL_RELAY_HOST=smtp.dreamteclabs.com
NEXUS_MAIL_RELAY_IPV4=23.254.215.34
NEXUS_WEBMAIL_HTTP_ORIGIN=http://192.168.0.29:80
NEXUS_WEBMAIL_HTTPS_ORIGIN=https://192.168.0.29:443
NEXUS_DMARC_POLICY=p=none
```

Do not commit the real environment file or its values to Git.

### Cloudflare token

Use a dedicated API token with the minimum permissions necessary for the zones and tunnel Nexus manages. The implementation needs zone/DNS read-write access and Cloudflare Tunnel configuration read-write access. Scope the token to the DreamtecLabs account and only the managed zones whenever Cloudflare's token controls allow it.

### SSH access

Use dedicated SSH keys for Nexus automation. Prefer command restrictions/sudo policy on the target systems rather than unconstrained interactive keys.

For the DDNS host, Nexus needs only the ability to:

1. read and append the configured `records.conf`;
2. start `cloudflare-ddns.service`.

For Hestia, Nexus invokes Hestia CLI commands under `/usr/local/hestia/bin`. The onboarding implementation currently uses:

- `v-list-mail-domain`
- `v-add-mail-domain`
- `v-list-mail-domain-dkim-dns`
- `v-add-mail-domain-dkim`
- `v-add-mail-domain-webmail`
- `v-add-letsencrypt-domain`

## Idempotent mail onboarding

For a new mail-enabled domain the privileged helper performs this sequence:

1. Validate the domain name.
2. Verify `webmail.<domain>` is not controlled by DDNS.
3. Add `domain|mail.domain|false` to the DDNS central configuration if absent and trigger the DDNS service.
4. Resolve the Cloudflare zone.
5. Upsert `webmail.<domain>` as a proxied CNAME for the configured tunnel.
6. Put the Tunnel ingress origin temporarily on `HTTP -> 192.168.0.29:80`.
7. Create the Hestia mail domain if absent.
8. Enable Roundcube for newly created mail domains and ensure DKIM exists.
9. Wait for the DDNS-owned `mail.<domain>` A record to resolve. Nexus does **not** take over that A record through the Cloudflare API.
10. Ask Hestia to issue the mail/webmail Let's Encrypt certificate.
11. Upsert MX.
12. Upsert only the TXT record beginning with `v=spf1`; unrelated TXT verification records are preserved.
13. Read the DKIM public key from Hestia and publish it.
14. Publish DMARC using the configured initial policy, default `p=none`.
15. Switch the Tunnel origin to `HTTPS -> 192.168.0.29:443` and set `No TLS Verify=true`.
16. Run final DNS/TLS/mail validation and return the result to the UI.

The helper refuses ambiguous/destructive DNS updates, including duplicate SPF records, duplicate managed TXT records, multiple same-type records where ownership is unclear, and an existing MX that points somewhere other than `mail.<domain>`.

## Existing DDNS exception to resolve

The historical DDNS list includes `webmail.dreamtec.com.br`. This conflicts with the new ownership rule. Nexus will intentionally refuse Tunnel onboarding for `dreamtec.com.br` until that hostname is removed from central DDNS and is ready to become Tunnel-managed.

No automatic deletion is performed.

## Recovery and rollback

Onboarding is staged so failures are visible and rerunnable. Most operations are upserts or existence checks.

If certificate issuance fails while webmail is temporarily routed to HTTP:

1. Keep or restore the Tunnel ingress for `webmail.<domain>` to the HTTP origin.
2. Confirm `mail.<domain>` resolves to the current public residential IP.
3. Confirm Hestia owns the mail domain and Roundcube hostname.
4. Retry certificate issuance/onboarding.
5. Only after successful issuance switch the Tunnel origin to HTTPS/443 with `No TLS Verify=true`.

If DNS publication fails after Hestia was created, rerun onboarding. The helper will reuse the existing Hestia domain/DKIM and only update the DNS records it owns.

If a manual rollback is required, do not delete a domain from Hestia as a first step. Restore DNS/Tunnel ownership first, verify mail delivery, and perform destructive deletion only as a separate reviewed change. The Nexus onboarding endpoint itself does not delete domains, mailboxes, Tunnel routes, or DNS records.

## Security guarantees

- Provider secrets remain in a root-only server-side environment file.
- The UI receives operational state only.
- Cloudflare token values are not logged or returned.
- Mailbox passwords are never read or returned.
- The helper uses `BatchMode=yes` for SSH so automation cannot stop on an interactive password prompt.
- Read-only validation is separated from modify privilege.
- Onboarding never proxies `mail.*` or `smtp.*`.
- Onboarding never writes `webmail.*` to central DDNS.
- The DDNS-managed mail A record remains owned by the central DDNS process.
- Conflicting DNS states fail closed instead of being silently deleted/replaced.

## Scope of this delivery and extension seam

This delivery establishes the real Nexus module, source-of-truth model, live health validation, audit trail, secure provider execution seam, and the full mail/webmail onboarding path used by the current infrastructure.

The same backend boundary is intended for the next provider operations without putting provider logic into Yew/browser code: website/domain lifecycle in Hestia, mailbox CRUD/quota, Cloudflare DNS record CRUD, internal DNS adapters, tunnel inventory, certificate-expiry scheduling, and alert fan-out. Destructive operations should remain separate endpoints with explicit confirmation rather than being added to the onboarding helper.

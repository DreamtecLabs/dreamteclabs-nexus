# Nexus Domains reconciliation

The Domains & Hosting UI treats incomplete mail domains as a desired-state reconciliation problem rather than a manual troubleshooting exercise.

## Operator flow

1. **Validate** performs the existing read-only DNS/TLS/mail checks.
2. A domain that does not pass all required checks is shown as **Incomplete** together with the number of configured checks.
3. The row exposes **Fix configuration**.
4. Fix configuration calls the existing privileged `/domains/onboard` endpoint. The backend helper is idempotent: existing Hestia mail domains, Roundcube, DKIM and owned Cloudflare/DDNS state are reused or upserted instead of blindly recreated.
5. The endpoint performs its final validation and the UI immediately replaces the row health state with that returned validation result.
6. If reconciliation succeeds but a live check still fails, the UI keeps the domain in **Incomplete** state instead of falsely declaring success.
7. If the helper detects ambiguous ownership or conflicting managed DNS, it fails closed and the UI reports **Repair stopped safely**.

The same reconciler is used by **Complete setup** for a new or partially configured domain. This intentionally avoids separate create-versus-repair code paths.

## Required checks shown in the UI

The configuration progress counter uses the backend validation keys directly:

- `mail_a`
- `mx`
- `spf`
- `dkim`
- `dmarc`
- `smtp_submission`
- `imap_tls`
- `webmail_tunnel_dns`
- `webmail_tls`

A domain is shown as **Active · Healthy** only when the backend returns `healthy=true`.

## Safety

One-click repair does not weaken the existing ownership rules. In particular:

- `mail.<domain>` remains DDNS-owned and DNS-only;
- `webmail.<domain>` remains Tunnel-owned and may not also be DDNS-managed;
- conflicting MX/TXT states are not silently deleted or replaced;
- provider secrets remain server-side;
- validation remains a separate read-only action;
- destructive removal of domains, mailboxes, DNS records or Tunnel routes is outside this flow.

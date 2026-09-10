from __future__ import annotations

import asyncio
import ssl

import dns.asyncresolver
import dns.exception
import httpx

from nexus_core.ports.domains import DomainCheck, DomainRecord, DomainValidation


class PublicDomainDiagnosticsProvider:
    def __init__(self, timeout_seconds: float = 5.0) -> None:
        self._timeout = timeout_seconds

    async def _dns(self, name: str, record_type: str) -> tuple[bool, str]:
        try:
            answers = await dns.asyncresolver.resolve(name, record_type, lifetime=self._timeout)
            values = [str(answer).strip() for answer in answers]
            return bool(values), " | ".join(values)[:500]
        except (dns.exception.DNSException, OSError) as exc:
            return False, type(exc).__name__

    async def _tcp(self, host: str, port: int, *, tls: bool = False) -> tuple[bool, str]:
        context = ssl.create_default_context() if tls else None
        try:
            reader, writer = await asyncio.wait_for(
                asyncio.open_connection(host, port, ssl=context, server_hostname=host if tls else None),
                timeout=self._timeout,
            )
            if tls:
                # Mail protocols (IMAP, POP3, ...) push a greeting banner immediately after
                # the TLS handshake completes. Closing before draining it races the server's
                # write against our close_notify and OpenSSL raises APPLICATION_DATA_AFTER_
                # CLOSE_NOTIFY even though the handshake (and certificate) were fine.
                try:
                    await asyncio.wait_for(reader.read(1), timeout=self._timeout)
                except (TimeoutError, OSError):
                    pass
            writer.close()
            await writer.wait_closed()
            return True, f"{host}:{port} reachable"
        except (OSError, TimeoutError, ssl.SSLError) as exc:
            return False, type(exc).__name__

    async def _https(self, host: str) -> tuple[bool, str]:
        try:
            async with httpx.AsyncClient(timeout=self._timeout, follow_redirects=True) as client:
                response = await client.get(f"https://{host}/")
            return response.status_code < 500, f"HTTP {response.status_code}"
        except httpx.HTTPError as exc:
            return False, type(exc).__name__

    async def validate(self, domain: DomainRecord) -> DomainValidation:
        checks: list[DomainCheck] = []
        mail_host = f"mail.{domain.name}"
        webmail_host = f"webmail.{domain.name}"
        if domain.mail:
            a_ok, a_detail = await self._dns(mail_host, "A")
            checks.append(DomainCheck("mail_a", "Mail A record", a_ok, a_detail or "record not found"))
            mx_ok, mx_detail = await self._dns(domain.name, "MX")
            checks.append(DomainCheck("mx", "MX", mx_ok and mail_host.lower() in mx_detail.lower(), mx_detail or "record not found"))
            spf_ok, spf_detail = await self._dns(domain.name, "TXT")
            checks.append(DomainCheck("spf", "SPF", spf_ok and "v=spf1" in spf_detail.lower(), spf_detail or "record not found"))
            dkim_ok, dkim_detail = await self._dns(f"mail._domainkey.{domain.name}", "TXT")
            checks.append(DomainCheck("dkim", "DKIM", dkim_ok and "v=dkim1" in dkim_detail.lower(), dkim_detail or "record not found"))
            dmarc_ok, dmarc_detail = await self._dns(f"_dmarc.{domain.name}", "TXT")
            checks.append(DomainCheck("dmarc", "DMARC", dmarc_ok and "v=dmarc1" in dmarc_detail.lower(), dmarc_detail or "record not found"))
            smtp_ok, smtp_detail = await self._tcp(mail_host, 587)
            checks.append(DomainCheck("smtp_submission", "SMTP submission", smtp_ok, smtp_detail))
            imap_ok, imap_detail = await self._tcp(mail_host, 993, tls=True)
            checks.append(DomainCheck("imap_tls", "IMAP TLS", imap_ok, imap_detail))
        if domain.webmail:
            web_dns_ok, web_dns_detail = await self._dns(webmail_host, "A")
            if not web_dns_ok:
                web_dns_ok, web_dns_detail = await self._dns(webmail_host, "CNAME")
            checks.append(DomainCheck("webmail_dns", "Webmail DNS", web_dns_ok, web_dns_detail or "record not found"))
            web_tls_ok, web_tls_detail = await self._https(webmail_host)
            checks.append(DomainCheck("webmail_tls", "Webmail TLS", web_tls_ok, web_tls_detail))
        return DomainValidation(domain=domain.name, healthy=all(check.ok for check in checks), checks=tuple(checks))

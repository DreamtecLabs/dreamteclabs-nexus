use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Error, bail};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use proxmox_router::{Permission, Router, SubdirMap, list_subdirs_api_method};
use proxmox_schema::api;
use proxmox_sortable_macro::sortable;

use pdm_api_types::{PRIV_SYS_AUDIT, PRIV_SYS_MODIFY};
use pdm_buildcfg::configdir;

const INVENTORY_FILENAME: &str = configdir!("/domains-hosting.json");
const AUDIT_FILENAME: &str = configdir!("/domains-hosting-audit.log");
const DEFAULT_HELPER: &str = "/usr/libexec/proxmox/nexus-domains-helper";

#[sortable]
const SUBDIRS: SubdirMap = &sorted!([
    ("onboard", &Router::new().post(&API_METHOD_ONBOARD_DOMAIN)),
    ("validate", &Router::new().post(&API_METHOD_VALIDATE_DOMAIN)),
]);

pub const ROUTER: Router = Router::new()
    .get(&API_METHOD_GET_INVENTORY)
    .subdirs(SUBDIRS);

fn default_inventory() -> Value {
    json!({
        "version": 1,
        "defaults": {
            "hestia_host": "192.168.0.29",
            "hestia_user": "admin",
            "relay_hostname": "smtp.dreamteclabs.com",
            "relay_ipv4": "23.254.215.34",
            "webmail_origin_http": "http://192.168.0.29:80",
            "webmail_origin_https": "https://192.168.0.29:443",
            "webmail_no_tls_verify": true,
            "dmarc_policy": "p=none"
        },
        "domains": [
            {"name":"dreamteclabs.com","mail":true,"webmail":true,"ddns":true,"tunnel":true},
            {"name":"kinpilot.app","mail":true,"webmail":true,"ddns":true,"tunnel":true},
            {"name":"savipilot.com","mail":true,"webmail":true,"ddns":true,"tunnel":true},
            {"name":"domuspilot.com","mail":true,"webmail":true,"ddns":true,"tunnel":true},
            {"name":"mundoleo.co","mail":true,"webmail":true,"ddns":true,"tunnel":true},
            {"name":"dreamtec.com.br","mail":true,"webmail":true,"ddns":true,"tunnel":false},
            {"name":"claudiokaist.com","mail":false,"webmail":false,"ddns":true,"tunnel":false}
        ],
        "policy": {
            "mail_hostnames_dns_only": true,
            "smtp_hostnames_dns_only": true,
            "webmail_hostnames_tunnel_managed": true,
            "forbid_webmail_ddns": true,
            "forbid_mail_proxy": true
        }
    })
}

fn read_inventory() -> Result<Value, Error> {
    let Some(raw) = proxmox_sys::fs::file_read_optional_string(INVENTORY_FILENAME)? else {
        return Ok(default_inventory());
    };

    serde_json::from_str(&raw).context("unable to parse domains-hosting.json")
}

async fn command_output(program: &str, args: &[&str]) -> Result<String, Error> {
    let mut command = Command::new(program);
    command.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = tokio::time::timeout(Duration::from_secs(12), command.output())
        .await
        .with_context(|| format!("{program} timed out"))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("{program} failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return Ok(stdout);
    }
    Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

async fn dig(record_type: &str, name: &str) -> Value {
    match command_output("dig", &["+short", record_type, name]).await {
        Ok(output) if !output.is_empty() => json!({"ok":true,"value":output}),
        Ok(_) => json!({"ok":false,"value":"","error":"record not found"}),
        Err(err) => json!({"ok":false,"value":"","error":err.to_string()}),
    }
}

fn txt_contains(value: &Value, needle: &str) -> bool {
    value
        .get("value")
        .and_then(Value::as_str)
        .is_some_and(|value| value.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()))
}

fn txt_prefix_count(value: &Value, prefix: &str) -> usize {
    value
        .get("value")
        .and_then(Value::as_str)
        .map(|value| {
            value
                .lines()
                .filter(|line| line.trim_matches('"').to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()))
                .count()
        })
        .unwrap_or(0)
}

async fn openssl_check(host: &str, port: u16, starttls: Option<&str>) -> Value {
    let connect = format!("{host}:{port}");
    let mut args = vec![
        "s_client".to_string(),
        "-connect".to_string(),
        connect,
        "-servername".to_string(),
        host.to_string(),
        "-verify_return_error".to_string(),
        "-brief".to_string(),
    ];
    if let Some(protocol) = starttls {
        args.push("-starttls".to_string());
        args.push(protocol.to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match command_output("openssl", &refs).await {
        Ok(output) => json!({"ok":true,"endpoint":format!("{host}:{port}"),"summary":output}),
        Err(err) => json!({"ok":false,"endpoint":format!("{host}:{port}"),"error":err.to_string()}),
    }
}

async fn validate_domain_inner(domain: &str) -> Value {
    let mail = format!("mail.{domain}");
    let webmail = format!("webmail.{domain}");
    let dmarc = format!("_dmarc.{domain}");
    let dkim = format!("mail._domainkey.{domain}");

    let (mail_a, webmail_dns, mx, spf, dkim_txt, dmarc_txt, smtp, imap, webmail_tls) = tokio::join!(
        dig("A", &mail),
        dig("A", &webmail),
        dig("MX", domain),
        dig("TXT", domain),
        dig("TXT", &dkim),
        dig("TXT", &dmarc),
        openssl_check(&mail, 587, Some("smtp")),
        openssl_check(&mail, 993, None),
        openssl_check(&webmail, 443, None),
    );

    let mx_ok = mx
        .get("value")
        .and_then(Value::as_str)
        .is_some_and(|value| value.trim_end_matches('.').contains(&mail));
    let spf_count = txt_prefix_count(&spf, "v=spf1");
    let spf_ok = spf_count == 1;
    let dkim_ok = txt_contains(&dkim_txt, "p=");
    let dmarc_ok = txt_prefix_count(&dmarc_txt, "v=dmarc1") == 1;

    let healthy = mail_a.get("ok").and_then(Value::as_bool).unwrap_or(false)
        && webmail_dns.get("ok").and_then(Value::as_bool).unwrap_or(false)
        && mx_ok
        && spf_ok
        && dkim_ok
        && dmarc_ok
        && smtp.get("ok").and_then(Value::as_bool).unwrap_or(false)
        && imap.get("ok").and_then(Value::as_bool).unwrap_or(false)
        && webmail_tls.get("ok").and_then(Value::as_bool).unwrap_or(false);

    json!({
        "domain": domain,
        "healthy": healthy,
        "checks": {
            "mail_a": mail_a,
            "webmail_tunnel_dns": webmail_dns,
            "mx": {"ok":mx_ok,"detail":mx},
            "spf": {"ok":spf_ok,"count":spf_count,"detail":spf},
            "dkim": {"ok":dkim_ok,"detail":dkim_txt},
            "dmarc": {"ok":dmarc_ok,"detail":dmarc_txt},
            "smtp_submission": smtp,
            "imap_tls": imap,
            "webmail_tls": webmail_tls
        }
    })
}

async fn append_audit(action: &str, domain: &str, outcome: &str) -> Result<(), Error> {
    let safe_outcome = outcome
        .replace('\n', " ")
        .replace('\r', " ")
        .replace('\t', " ");
    let line = format!(
        "{}\taction={}\tdomain={}\toutcome={}\n",
        proxmox_time::epoch_i64(),
        action,
        domain,
        safe_outcome
    );
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(AUDIT_FILENAME)
        .await?;
    file.write_all(line.as_bytes()).await?;
    Ok(())
}

#[api(
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_AUDIT, false),
    },
    protected: true,
)]
/// Return the Nexus source-of-truth inventory and policy for domains and hosting.
pub fn get_inventory() -> Result<Value, Error> {
    read_inventory()
}

#[api(
    input: {
        properties: {
            domain: { type: String },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_AUDIT, false),
    },
    protected: true,
)]
/// Perform live DNS, TLS and mail connectivity checks without changing infrastructure.
pub async fn validate_domain(domain: String) -> Result<Value, Error> {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty() || domain.contains('/') || domain.contains(' ') {
        bail!("invalid domain");
    }
    let result = validate_domain_inner(&domain).await;
    let outcome = if result.get("healthy").and_then(Value::as_bool).unwrap_or(false) {
        "healthy"
    } else {
        "degraded"
    };
    append_audit("validate", &domain, outcome).await?;
    Ok(result)
}

#[api(
    input: {
        properties: {
            domain: { type: String },
            hestia_user: {
                type: String,
                optional: true,
            },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_MODIFY, false),
    },
    protected: true,
)]
/// Run the idempotent domain onboarding helper. Secrets are read by the helper from its root-only environment file.
pub async fn onboard_domain(domain: String, hestia_user: Option<String>) -> Result<Value, Error> {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty() || domain.contains('/') || domain.contains(' ') {
        bail!("invalid domain");
    }

    let helper = std::env::var("NEXUS_DOMAINS_HELPER").unwrap_or_else(|_| DEFAULT_HELPER.to_string());
    if !Path::new(&helper).exists() {
        bail!("domains helper is not installed at {helper}");
    }

    let user = hestia_user.unwrap_or_else(|| "admin".to_string());
    let output = Command::new(&helper)
        .arg("onboard")
        .arg(&domain)
        .arg(&user)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("failed to start domains onboarding helper")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        append_audit("onboard", &domain, &format!("failed: {stderr}")).await?;
        bail!("domain onboarding failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let helper_result: Value = serde_json::from_str(&stdout)
        .with_context(|| format!("domains helper returned invalid JSON: {stdout}"))?;
    let validation = validate_domain_inner(&domain).await;
    append_audit("onboard", &domain, "completed").await?;

    Ok(json!({
        "domain": domain,
        "result": helper_result,
        "validation": validation
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_inventory_enforces_mail_policy() {
        let inventory = default_inventory();
        assert_eq!(inventory["policy"]["forbid_mail_proxy"], true);
        assert_eq!(inventory["policy"]["forbid_webmail_ddns"], true);
    }

    #[test]
    fn bootstrap_inventory_contains_current_managed_domains() {
        let inventory = default_inventory();
        let domains = inventory["domains"].as_array().unwrap();
        assert!(domains.iter().any(|domain| domain["name"] == "mundoleo.co"));
        assert!(domains.iter().any(|domain| domain["name"] == "kinpilot.app"));
    }

    #[test]
    fn spf_duplicate_detection_requires_exactly_one_record() {
        let one = json!({"value":"\"v=spf1 a mx ~all\"\n\"google-site-verification=x\""});
        let two = json!({"value":"\"v=spf1 a mx ~all\"\n\"v=spf1 ip4:192.0.2.1 ~all\""});
        assert_eq!(txt_prefix_count(&one, "v=spf1"), 1);
        assert_eq!(txt_prefix_count(&two, "v=spf1"), 2);
    }
}

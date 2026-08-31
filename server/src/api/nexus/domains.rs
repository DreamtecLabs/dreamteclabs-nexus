use std::path::Path;
use std::process::Stdio;
use std::sync::Mutex;
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
const HELPER_TIMEOUT: Duration = Duration::from_secs(300);

static INVENTORY_WRITE_LOCK: Mutex<()> = Mutex::new(());

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

fn normalize_domain(input: &str) -> Result<String, Error> {
    let domain = input.trim().trim_end_matches('.').to_ascii_lowercase();
    validate_domain_name(&domain)?;
    Ok(domain)
}

fn validate_domain_name(domain: &str) -> Result<(), Error> {
    if domain.len() > 253 || !domain.contains('.') {
        bail!("invalid domain");
    }

    let mut labels = domain.split('.').peekable();
    let mut last_label = None;
    while let Some(label) = labels.next() {
        if label.is_empty() || label.len() > 63 {
            bail!("invalid domain");
        }
        let bytes = label.as_bytes();
        if !bytes.first().is_some_and(u8::is_ascii_alphanumeric)
            || !bytes.last().is_some_and(u8::is_ascii_alphanumeric)
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        {
            bail!("invalid domain");
        }
        last_label = Some(label);
    }

    let tld = last_label.context("invalid domain")?;
    if tld.len() < 2 || !tld.bytes().all(|byte| byte.is_ascii_lowercase()) {
        bail!("invalid domain");
    }

    Ok(())
}

fn validate_hestia_user(user: &str) -> Result<(), Error> {
    if user.is_empty() || user.len() > 64 {
        bail!("invalid Hestia user");
    }
    let bytes = user.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-'))
    {
        bail!("invalid Hestia user");
    }
    Ok(())
}

fn read_inventory() -> Result<Value, Error> {
    let Some(raw) = proxmox_sys::fs::file_read_optional_string(INVENTORY_FILENAME)? else {
        return Ok(default_inventory());
    };

    serde_json::from_str(&raw).context("unable to parse domains-hosting.json")
}

fn upsert_mail_domain(inventory: &mut Value, domain: &str) -> Result<bool, Error> {
    let domains = inventory
        .get_mut("domains")
        .and_then(Value::as_array_mut)
        .context("domains-hosting inventory is missing a domains array")?;

    if let Some(existing) = domains
        .iter_mut()
        .find(|entry| entry.get("name").and_then(Value::as_str) == Some(domain))
    {
        existing["mail"] = json!(true);
        existing["webmail"] = json!(true);
        existing["ddns"] = json!(true);
        existing["tunnel"] = json!(true);
        return Ok(false);
    }

    domains.push(json!({
        "name": domain,
        "mail": true,
        "webmail": true,
        "ddns": true,
        "tunnel": true
    }));
    Ok(true)
}

fn persist_mail_domain(domain: &str) -> Result<(), Error> {
    let _guard = INVENTORY_WRITE_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("domains inventory write lock is poisoned"))?;
    let mut inventory = read_inventory()?;
    upsert_mail_domain(&mut inventory, domain)?;

    let temporary = format!("{INVENTORY_FILENAME}.{}.tmp", std::process::id());
    let contents = serde_json::to_vec_pretty(&inventory)?;
    std::fs::write(&temporary, contents).context("unable to write temporary domains inventory")?;
    std::fs::rename(&temporary, INVENTORY_FILENAME)
        .context("unable to atomically replace domains inventory")?;
    Ok(())
}

async fn command_output(program: &str, args: &[&str]) -> Result<String, Error> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
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
        .is_some_and(|value| {
            value
                .to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        })
}

fn txt_prefix_count(value: &Value, prefix: &str) -> usize {
    value
        .get("value")
        .and_then(Value::as_str)
        .map(|value| {
            value
                .lines()
                .filter(|line| {
                    line.trim_matches('"')
                        .to_ascii_lowercase()
                        .starts_with(&prefix.to_ascii_lowercase())
                })
                .count()
        })
        .unwrap_or(0)
}

fn mx_points_only_to(value: &Value, expected_host: &str) -> bool {
    let Some(value) = value.get("value").and_then(Value::as_str) else {
        return false;
    };
    let mut lines = value.lines().filter(|line| !line.trim().is_empty());
    let Some(line) = lines.next() else {
        return false;
    };
    if lines.next().is_some() {
        return false;
    }
    let Some(target) = line.split_whitespace().nth(1) else {
        return false;
    };
    target
        .trim_end_matches('.')
        .eq_ignore_ascii_case(expected_host)
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

    let mx_ok = mx_points_only_to(&mx, &mail);
    let spf_count = txt_prefix_count(&spf, "v=spf1");
    let spf_ok = spf_count == 1;
    let dkim_ok = txt_contains(&dkim_txt, "p=");
    let dmarc_ok = txt_prefix_count(&dmarc_txt, "v=dmarc1") == 1;

    let healthy = mail_a.get("ok").and_then(Value::as_bool).unwrap_or(false)
        && webmail_dns
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        && mx_ok
        && spf_ok
        && dkim_ok
        && dmarc_ok
        && smtp.get("ok").and_then(Value::as_bool).unwrap_or(false)
        && imap.get("ok").and_then(Value::as_bool).unwrap_or(false)
        && webmail_tls
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);

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
            domain: {
                description: "Domain name to validate.",
                type: String,
            },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_AUDIT, false),
    },
    protected: true,
)]
/// Perform live DNS, TLS and mail connectivity checks without changing infrastructure.
pub async fn validate_domain(domain: String) -> Result<Value, Error> {
    let domain = normalize_domain(&domain)?;
    let result = validate_domain_inner(&domain).await;
    let outcome = if result
        .get("healthy")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
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
            domain: {
                description: "Domain name to onboard.",
                type: String,
            },
            hestia_user: {
                description: "Hestia account that owns the mail domain.",
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
    let domain = normalize_domain(&domain)?;

    let helper =
        std::env::var("NEXUS_DOMAINS_HELPER").unwrap_or_else(|_| DEFAULT_HELPER.to_string());
    if !Path::new(&helper).exists() {
        bail!("domains helper is not installed at {helper}");
    }

    let user = hestia_user.unwrap_or_else(|| "admin".to_string());
    validate_hestia_user(&user)?;

    let mut command = Command::new(&helper);
    command
        .arg("onboard")
        .arg(&domain)
        .arg(&user)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let output = match tokio::time::timeout(HELPER_TIMEOUT, command.output()).await {
        Ok(output) => output.context("failed to start domains onboarding helper")?,
        Err(_) => {
            append_audit("onboard", &domain, "failed: helper timed out").await?;
            bail!("domain onboarding helper timed out");
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        append_audit("onboard", &domain, &format!("failed: {stderr}")).await?;
        bail!("domain onboarding failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let helper_result: Value = serde_json::from_str(&stdout)
        .with_context(|| format!("domains helper returned invalid JSON: {stdout}"))?;

    if let Err(err) = persist_mail_domain(&domain) {
        append_audit(
            "onboard",
            &domain,
            &format!("providers completed; inventory persistence failed: {err}"),
        )
        .await?;
        return Err(err);
    }

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
        assert!(
            domains
                .iter()
                .any(|domain| domain["name"] == "kinpilot.app")
        );
    }

    #[test]
    fn domain_and_hestia_user_validation_rejects_shell_like_input() {
        assert_eq!(normalize_domain("Example.COM.").unwrap(), "example.com");
        assert!(normalize_domain("example.com;id").is_err());
        assert!(normalize_domain("-bad.example").is_err());
        assert!(normalize_domain("localhost").is_err());
        assert!(validate_hestia_user("admin").is_ok());
        assert!(validate_hestia_user("ops-user_1").is_ok());
        assert!(validate_hestia_user("admin';id").is_err());
        assert!(validate_hestia_user(" bad").is_err());
    }

    #[test]
    fn spf_duplicate_detection_requires_exactly_one_record() {
        let one = json!({"value":"\"v=spf1 a mx ~all\"\n\"google-site-verification=x\""});
        let two = json!({"value":"\"v=spf1 a mx ~all\"\n\"V=SPF1 ip4:192.0.2.1 ~all\""});
        assert_eq!(txt_prefix_count(&one, "v=spf1"), 1);
        assert_eq!(txt_prefix_count(&two, "v=spf1"), 2);
    }

    #[test]
    fn mx_validation_requires_one_exact_target() {
        let good = json!({"value":"10 mail.example.com."});
        let wrong = json!({"value":"10 mail.example.com.evil.invalid."});
        let duplicate = json!({"value":"10 mail.example.com.\n20 mail.example.com."});
        assert!(mx_points_only_to(&good, "mail.example.com"));
        assert!(!mx_points_only_to(&wrong, "mail.example.com"));
        assert!(!mx_points_only_to(&duplicate, "mail.example.com"));
    }

    #[test]
    fn onboarding_inventory_upsert_is_idempotent() {
        let mut inventory = json!({"domains":[]});
        assert!(upsert_mail_domain(&mut inventory, "example.com").unwrap());
        assert!(!upsert_mail_domain(&mut inventory, "example.com").unwrap());
        let domains = inventory["domains"].as_array().unwrap();
        assert_eq!(domains.len(), 1);
        assert_eq!(domains[0]["name"], "example.com");
        assert_eq!(domains[0]["mail"], true);
        assert_eq!(domains[0]["webmail"], true);
        assert_eq!(domains[0]["ddns"], true);
        assert_eq!(domains[0]["tunnel"], true);
    }
}

use std::path::Path;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Error, bail};
use serde_json::{Map, Value, json};
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
const HELPER_RECONCILE_ATTEMPTS: usize = 3;
const HELPER_RETRY_DELAY: Duration = Duration::from_secs(3);
const REQUIRED_CHECK_KEYS: [&str; 9] = [
    "mail_a",
    "mx",
    "spf",
    "dkim",
    "dmarc",
    "smtp_submission",
    "imap_tls",
    "webmail_tunnel_dns",
    "webmail_tls",
];
const ADOPTABLE_DNS_CHECKS: [&str; 4] = ["mx", "spf", "dkim", "dmarc"];

static INVENTORY_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[sortable]
const SUBDIRS: SubdirMap = &sorted!([
    ("adopt", &Router::new().post(&API_METHOD_ADOPT_EXISTING_DOMAIN)),
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

fn write_inventory(inventory: &Value) -> Result<(), Error> {
    let temporary = format!("{INVENTORY_FILENAME}.{}.tmp", std::process::id());
    let contents = serde_json::to_vec_pretty(inventory)?;
    std::fs::write(&temporary, contents).context("unable to write temporary domains inventory")?;
    std::fs::rename(&temporary, INVENTORY_FILENAME)
        .context("unable to atomically replace domains inventory")?;
    Ok(())
}

fn find_domain_entry<'a>(inventory: &'a Value, domain: &str) -> Option<&'a Value> {
    inventory
        .get("domains")
        .and_then(Value::as_array)
        .and_then(|domains| {
            domains
                .iter()
                .find(|entry| entry.get("name").and_then(Value::as_str) == Some(domain))
        })
}

fn find_domain_entry_mut<'a>(inventory: &'a mut Value, domain: &str) -> Option<&'a mut Value> {
    inventory
        .get_mut("domains")
        .and_then(Value::as_array_mut)
        .and_then(|domains| {
            domains
                .iter_mut()
                .find(|entry| entry.get("name").and_then(Value::as_str) == Some(domain))
        })
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
        existing["configuration_mode"] = json!("nexus");
        if let Some(object) = existing.as_object_mut() {
            object.remove("adopted_checks");
        }
        return Ok(false);
    }

    domains.push(json!({
        "name": domain,
        "mail": true,
        "webmail": true,
        "ddns": true,
        "tunnel": true,
        "configuration_mode": "nexus"
    }));
    Ok(true)
}

fn persist_mail_domain(domain: &str) -> Result<(), Error> {
    let _guard = INVENTORY_WRITE_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("domains inventory write lock is poisoned"))?;
    let mut inventory = read_inventory()?;
    upsert_mail_domain(&mut inventory, domain)?;
    write_inventory(&inventory)
}

fn persist_adopted_domain(domain: &str, adopted_checks: &Map<String, Value>) -> Result<(), Error> {
    let _guard = INVENTORY_WRITE_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("domains inventory write lock is poisoned"))?;
    let mut inventory = read_inventory()?;
    let entry = find_domain_entry_mut(&mut inventory, domain)
        .context("domain is not present in the Nexus inventory")?;
    entry["configuration_mode"] = json!("existing");
    entry["adopted_checks"] = Value::Object(adopted_checks.clone());
    write_inventory(&inventory)
}

fn helper_exit_code_is_retryable(code: Option<i32>) -> bool {
    !matches!(code, Some(3) | Some(5) | Some(42) | Some(43))
}

fn helper_failure_detail(stdout: &[u8], stderr: &[u8], code: Option<i32>) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }
    match code {
        Some(code) => format!("helper exited with code {code} without diagnostic output"),
        None => "helper terminated without diagnostic output".to_string(),
    }
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

fn canonical_dns_lines<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    let mut lines: Vec<String> = lines
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect();
    lines.sort_by_key(|line| line.to_ascii_lowercase());
    lines.join("\n")
}

fn record_fingerprint(result: &Value, key: &str) -> Option<String> {
    let value = result
        .get("checks")?
        .get(key)?
        .get("detail")?
        .get("value")?
        .as_str()?;
    let prefix = match key {
        "spf" => Some("v=spf1"),
        "dkim" => Some("v=dkim1"),
        "dmarc" => Some("v=dmarc1"),
        "mx" => None,
        _ => return None,
    };
    let fingerprint = canonical_dns_lines(value.lines().filter(|line| {
        prefix.is_none_or(|prefix| {
            line.trim_matches('"')
                .to_ascii_lowercase()
                .starts_with(prefix)
        })
    }));
    (!fingerprint.is_empty()).then_some(fingerprint)
}

fn adopted_dns_snapshots(result: &Value) -> Map<String, Value> {
    ADOPTABLE_DNS_CHECKS
        .iter()
        .filter_map(|key| {
            record_fingerprint(result, key).map(|fingerprint| ((*key).to_string(), json!(fingerprint)))
        })
        .collect()
}

fn check_ok(result: &Value, key: &str) -> bool {
    result
        .get("checks")
        .and_then(|checks| checks.get(key))
        .and_then(|check| check.get("ok"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn all_required_checks_healthy(result: &Value) -> bool {
    REQUIRED_CHECK_KEYS.iter().all(|key| check_ok(result, key))
}

fn existing_configuration_detected(result: &Value, domain_entry: Option<&Value>) -> bool {
    let failed_existing_record = ADOPTABLE_DNS_CHECKS
        .iter()
        .any(|key| !check_ok(result, key) && record_fingerprint(result, key).is_some());
    let unmanaged_mail = domain_entry
        .is_some_and(|entry| !entry.get("mail").and_then(Value::as_bool).unwrap_or(false))
        && ADOPTABLE_DNS_CHECKS
            .iter()
            .any(|key| record_fingerprint(result, key).is_some());
    failed_existing_record || unmanaged_mail
}

fn apply_existing_policy(mut result: Value, domain_entry: &Value) -> Value {
    let adopted_checks = domain_entry
        .get("adopted_checks")
        .and_then(Value::as_object);
    let mut changed = false;

    if let Some(adopted_checks) = adopted_checks {
        for (key, expected) in adopted_checks {
            let Some(expected) = expected.as_str() else {
                continue;
            };
            let current = record_fingerprint(&result, key);
            let matches = current.as_deref() == Some(expected);
            if let Some(check) = result
                .get_mut("checks")
                .and_then(Value::as_object_mut)
                .and_then(|checks| checks.get_mut(key))
            {
                check["ok"] = json!(matches);
                check["adopted"] = json!(true);
                check["policy"] = json!("existing");
            }
            changed |= !matches;
        }
    }

    let healthy = all_required_checks_healthy(&result);
    result["healthy"] = json!(healthy);
    result["configuration_mode"] = json!("existing");
    result["decision_required"] = json!(changed);
    result["existing_configuration"] = json!({
        "detected": true,
        "changed": changed,
        "records": adopted_dns_snapshots(&result),
        "message": if changed {
            "The adopted configuration changed. Keep the new live configuration or migrate to the Nexus standard."
        } else {
            "Existing configuration is adopted and monitored without being replaced."
        }
    });
    result
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

async fn validate_domain_raw(domain: &str) -> Value {
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

async fn validate_domain_inner(domain: &str) -> Result<Value, Error> {
    let mut result = validate_domain_raw(domain).await;
    let inventory = read_inventory()?;
    let domain_entry = find_domain_entry(&inventory, domain);

    if domain_entry
        .and_then(|entry| entry.get("configuration_mode"))
        .and_then(Value::as_str)
        == Some("existing")
    {
        return Ok(apply_existing_policy(
            result,
            domain_entry.expect("domain entry was present"),
        ));
    }

    let decision_required = existing_configuration_detected(&result, domain_entry);
    result["configuration_mode"] = json!("nexus");
    result["decision_required"] = json!(decision_required);
    if decision_required {
        result["existing_configuration"] = json!({
            "detected": true,
            "changed": false,
            "records": adopted_dns_snapshots(&result),
            "message": "Existing DNS/mail configuration was detected. Choose whether to keep it or migrate to the Nexus Hestia standard."
        });
    }
    Ok(result)
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
    let result = validate_domain_inner(&domain).await?;
    let outcome = if result
        .get("decision_required")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "decision-required"
    } else if result
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
                description: "Domain whose existing live DNS/mail configuration should be adopted.",
                type: String,
            },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_MODIFY, false),
    },
    protected: true,
)]
/// Persist the current external DNS/mail configuration as an explicit, monitored policy choice.
pub async fn adopt_existing_domain(domain: String) -> Result<Value, Error> {
    let domain = normalize_domain(&domain)?;
    let raw = validate_domain_raw(&domain).await;
    let adopted_checks = adopted_dns_snapshots(&raw);
    if adopted_checks.is_empty() {
        bail!("no existing DNS/mail configuration was detected to adopt");
    }

    persist_adopted_domain(&domain, &adopted_checks)?;
    let validation = validate_domain_inner(&domain).await?;
    append_audit("adopt-existing", &domain, "completed").await?;

    Ok(json!({
        "domain": domain,
        "configuration_mode": "existing",
        "validation": validation
    }))
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
            replace_existing: {
                description: "Explicitly replace conflicting existing DNS/mail records with the Nexus Hestia standard.",
                type: bool,
                optional: true,
            },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_MODIFY, false),
    },
    protected: true,
)]
/// Run the idempotent domain onboarding helper. Existing conflicts are replaced only with explicit opt-in.
pub async fn onboard_domain(
    domain: String,
    hestia_user: Option<String>,
    replace_existing: Option<bool>,
) -> Result<Value, Error> {
    let domain = normalize_domain(&domain)?;

    let helper =
        std::env::var("NEXUS_DOMAINS_HELPER").unwrap_or_else(|_| DEFAULT_HELPER.to_string());
    if !Path::new(&helper).exists() {
        bail!("domains helper is not installed at {helper}");
    }

    let user = hestia_user.unwrap_or_else(|| "admin".to_string());
    validate_hestia_user(&user)?;
    let replace_existing = replace_existing.unwrap_or(false);
    let helper_action = if replace_existing { "migrate" } else { "onboard" };
    let audit_action = if replace_existing { "migrate" } else { "onboard" };

    let mut helper_result: Option<Value> = None;
    let mut last_failure = String::new();

    for attempt in 1..=HELPER_RECONCILE_ATTEMPTS {
        let mut command = Command::new(&helper);
        command
            .arg(helper_action)
            .arg(&domain)
            .arg(&user)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let output = match tokio::time::timeout(HELPER_TIMEOUT, command.output()).await {
            Ok(output) => output.context("failed to start domains onboarding helper")?,
            Err(_) => {
                last_failure =
                    format!("helper timed out on attempt {attempt}/{HELPER_RECONCILE_ATTEMPTS}");
                if attempt < HELPER_RECONCILE_ATTEMPTS {
                    tokio::time::sleep(HELPER_RETRY_DELAY).await;
                    continue;
                }
                append_audit(audit_action, &domain, &format!("failed: {last_failure}")).await?;
                bail!("domain onboarding failed: {last_failure}");
            }
        };

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            helper_result = Some(
                serde_json::from_str(&stdout)
                    .with_context(|| format!("domains helper returned invalid JSON: {stdout}"))?,
            );
            if attempt > 1 {
                append_audit(
                    "onboard-retry",
                    &domain,
                    &format!("recovered on attempt {attempt}/{HELPER_RECONCILE_ATTEMPTS}"),
                )
                .await?;
            }
            break;
        }

        let code = output.status.code();
        last_failure = helper_failure_detail(&output.stdout, &output.stderr, code);
        let retryable = helper_exit_code_is_retryable(code);

        if retryable && attempt < HELPER_RECONCILE_ATTEMPTS {
            append_audit(
                "onboard-retry",
                &domain,
                &format!(
                    "attempt {attempt}/{HELPER_RECONCILE_ATTEMPTS} failed; retrying: {last_failure}"
                ),
            )
            .await?;
            tokio::time::sleep(HELPER_RETRY_DELAY).await;
            continue;
        }

        append_audit(audit_action, &domain, &format!("failed: {last_failure}")).await?;
        bail!("domain onboarding failed: {last_failure}");
    }

    let helper_result = helper_result.context(format!(
        "domain onboarding failed after {HELPER_RECONCILE_ATTEMPTS} attempts: {last_failure}"
    ))?;

    if let Err(err) = persist_mail_domain(&domain) {
        append_audit(
            audit_action,
            &domain,
            &format!("providers completed; inventory persistence failed: {err}"),
        )
        .await?;
        return Err(err);
    }

    let validation = validate_domain_inner(&domain).await?;
    append_audit(audit_action, &domain, "completed").await?;

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
    fn onboarding_inventory_upsert_is_idempotent_and_clears_adoption() {
        let mut inventory = json!({"domains":[]});
        assert!(upsert_mail_domain(&mut inventory, "example.com").unwrap());
        inventory["domains"][0]["configuration_mode"] = json!("existing");
        inventory["domains"][0]["adopted_checks"] = json!({"mx":"10 external.example."});
        assert!(!upsert_mail_domain(&mut inventory, "example.com").unwrap());
        let domains = inventory["domains"].as_array().unwrap();
        assert_eq!(domains.len(), 1);
        assert_eq!(domains[0]["name"], "example.com");
        assert_eq!(domains[0]["mail"], true);
        assert_eq!(domains[0]["webmail"], true);
        assert_eq!(domains[0]["ddns"], true);
        assert_eq!(domains[0]["tunnel"], true);
        assert_eq!(domains[0]["configuration_mode"], "nexus");
        assert!(domains[0].get("adopted_checks").is_none());
    }

    #[test]
    fn retry_policy_keeps_configuration_conflicts_fail_closed() {
        assert!(helper_exit_code_is_retryable(Some(1)));
        assert!(helper_exit_code_is_retryable(None));
        assert!(!helper_exit_code_is_retryable(Some(3)));
        assert!(!helper_exit_code_is_retryable(Some(5)));
        assert!(!helper_exit_code_is_retryable(Some(42)));
        assert!(!helper_exit_code_is_retryable(Some(43)));
    }

    #[test]
    fn adopted_dns_fingerprint_is_order_independent_and_prefix_scoped() {
        let validation = json!({"checks":{
            "mx":{"detail":{"value":"20 b.example.\n10 a.example."}},
            "spf":{"detail":{"value":"\"google-site-verification=x\"\n\"v=spf1 include:_spf.example ~all\""}},
            "dkim":{"detail":{"value":"\"v=DKIM1; p=abc\""}},
            "dmarc":{"detail":{"value":"\"v=DMARC1; p=none\""}}
        }});
        assert_eq!(
            record_fingerprint(&validation, "mx").as_deref(),
            Some("10 a.example.\n20 b.example.")
        );
        assert_eq!(
            record_fingerprint(&validation, "spf").as_deref(),
            Some("\"v=spf1 include:_spf.example ~all\"")
        );
    }

    #[test]
    fn existing_policy_can_adopt_nonstandard_records_without_masking_changes() {
        let result = json!({
            "healthy": false,
            "checks": {
                "mail_a":{"ok":true},
                "mx":{"ok":false,"detail":{"value":"10 external.example."}},
                "spf":{"ok":true,"detail":{"value":"\"v=spf1 ~all\""}},
                "dkim":{"ok":true,"detail":{"value":"\"v=DKIM1; p=abc\""}},
                "dmarc":{"ok":false,"detail":{"value":"\"v=DMARC1; p=reject\"\n\"v=DMARC1; p=none\""}},
                "smtp_submission":{"ok":true},
                "imap_tls":{"ok":true},
                "webmail_tunnel_dns":{"ok":true},
                "webmail_tls":{"ok":true}
            }
        });
        let snapshots = adopted_dns_snapshots(&result);
        let entry = json!({"configuration_mode":"existing","adopted_checks":snapshots});
        let adopted = apply_existing_policy(result, &entry);
        assert_eq!(adopted["healthy"], true);
        assert_eq!(adopted["checks"]["mx"]["adopted"], true);
        assert_eq!(adopted["checks"]["dmarc"]["adopted"], true);
        assert_eq!(adopted["decision_required"], false);
    }
}

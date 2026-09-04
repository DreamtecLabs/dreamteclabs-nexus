use std::net::IpAddr;
use std::path::Path;
use std::process::Stdio;
use std::sync::Mutex;

use anyhow::{Context, Error, bail};
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::process::Command;
use url::Url;

use proxmox_router::{Permission, Router, SubdirMap, list_subdirs_api_method};
use proxmox_schema::api;
use proxmox_sortable_macro::sortable;

use pdm_api_types::{PRIV_SYS_AUDIT, PRIV_SYS_MODIFY};
use pdm_buildcfg::configdir;

const INVENTORY_FILENAME: &str = configdir!("/monitoring.json");
const COLLECTOR_CONFIG_FILENAME: &str = configdir!("/nexus-icmp-collector.yaml");
const COLLECTOR_BINARY: &str = "/usr/bin/otelcol-contrib";
const COLLECTOR_SERVICE: &str = "nexus-icmp-collector.service";
const DEFAULT_SIGNOZ_URL: &str = "http://192.168.0.47:8080";
const DEFAULT_OTLP_ENDPOINT: &str = "192.168.0.47:4317";

static INVENTORY_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[sortable]
const SUBDIRS: SubdirMap = &sorted!([
    ("device", &Router::new().post(&API_METHOD_UPSERT_DEVICE)),
    (
        "device-delete",
        &Router::new().post(&API_METHOD_DELETE_DEVICE)
    ),
    (
        "device-probe",
        &Router::new().post(&API_METHOD_PROBE_DEVICE)
    ),
    ("reconcile", &Router::new().post(&API_METHOD_RECONCILE)),
    ("signoz", &Router::new().get(&API_METHOD_SIGNOZ_STATUS)),
]);

pub const ROUTER: Router = Router::new()
    .get(&API_METHOD_GET_MONITORING)
    .subdirs(SUBDIRS);

fn default_inventory() -> Value {
    json!({
        "version": 1,
        "signoz": {
            "url": DEFAULT_SIGNOZ_URL
        },
        "probe": {
            "otlp_endpoint": DEFAULT_OTLP_ENDPOINT,
            "collection_interval": "30s"
        },
        "devices": []
    })
}

fn read_inventory() -> Result<Value, Error> {
    let Some(raw) = proxmox_sys::fs::file_read_optional_string(INVENTORY_FILENAME)? else {
        return Ok(default_inventory());
    };
    serde_json::from_str(&raw).context("unable to parse monitoring.json")
}

fn write_inventory(inventory: &Value) -> Result<(), Error> {
    let temporary = format!("{INVENTORY_FILENAME}.{}.tmp", std::process::id());
    let contents = serde_json::to_vec_pretty(inventory)?;
    std::fs::write(&temporary, contents).context("unable to write temporary monitoring inventory")?;
    std::fs::rename(&temporary, INVENTORY_FILENAME)
        .context("unable to atomically replace monitoring inventory")?;
    Ok(())
}

fn normalize_name(input: &str) -> Result<String, Error> {
    let name = input.trim();
    if name.is_empty() || name.len() > 80 {
        bail!("device name must contain between 1 and 80 characters");
    }
    if name.chars().any(char::is_control) {
        bail!("device name contains invalid control characters");
    }
    Ok(name.to_string())
}

fn normalize_slug(input: &str) -> Result<String, Error> {
    let mut output = String::new();
    let mut previous_dash = false;
    for ch in input.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
            previous_dash = false;
        } else if !previous_dash && !output.is_empty() {
            output.push('-');
            previous_dash = true;
        }
    }
    while output.ends_with('-') {
        output.pop();
    }
    if output.is_empty() || output.len() > 64 {
        bail!("device name cannot be converted to a valid device id");
    }
    Ok(output)
}

fn normalize_simple_label(input: &str, field: &str) -> Result<String, Error> {
    let value = input.trim().to_ascii_lowercase();
    if value.is_empty() || value.len() > 64 {
        bail!("{field} must contain between 1 and 64 characters");
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("{field} contains invalid characters");
    }
    Ok(value)
}

fn normalize_address(input: &str) -> Result<String, Error> {
    let address = input.trim().trim_end_matches('.').to_ascii_lowercase();
    if address.parse::<IpAddr>().is_ok() {
        return Ok(address);
    }
    if address.is_empty() || address.len() > 253 {
        bail!("invalid device IP address or hostname");
    }
    for label in address.split('.') {
        if label.is_empty() || label.len() > 63 {
            bail!("invalid device IP address or hostname");
        }
        let bytes = label.as_bytes();
        if !bytes.first().is_some_and(u8::is_ascii_alphanumeric)
            || !bytes.last().is_some_and(u8::is_ascii_alphanumeric)
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        {
            bail!("invalid device IP address or hostname");
        }
    }
    Ok(address)
}

fn normalize_state(input: &str) -> Result<String, Error> {
    match input.trim().to_ascii_lowercase().as_str() {
        "enabled" => Ok("enabled".to_string()),
        "maintenance" => Ok("maintenance".to_string()),
        "disabled" => Ok("disabled".to_string()),
        _ => bail!("state must be enabled, maintenance or disabled"),
    }
}

fn yaml_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn enabled_devices(inventory: &Value) -> Vec<&Value> {
    inventory
        .get("devices")
        .and_then(Value::as_array)
        .map(|devices| {
            devices
                .iter()
                .filter(|device| device.get("state").and_then(Value::as_str) == Some("enabled"))
                .collect()
        })
        .unwrap_or_default()
}

fn collector_config(inventory: &Value) -> Result<String, Error> {
    let devices = enabled_devices(inventory);
    if devices.is_empty() {
        bail!("no enabled devices");
    }

    let interval = inventory
        .pointer("/probe/collection_interval")
        .and_then(Value::as_str)
        .unwrap_or("30s");
    let endpoint = inventory
        .pointer("/probe/otlp_endpoint")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_OTLP_ENDPOINT);

    let mut receivers = String::from("receivers:\n");
    let mut processors = String::from("processors:\n");
    let mut pipelines = String::new();

    for device in devices {
        let id = device
            .get("id")
            .and_then(Value::as_str)
            .context("monitoring device is missing id")?;
        let name = device
            .get("name")
            .and_then(Value::as_str)
            .context("monitoring device is missing name")?;
        let address = device
            .get("address")
            .and_then(Value::as_str)
            .context("monitoring device is missing address")?;
        let kind = device
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("device");
        let site = device
            .get("site")
            .and_then(Value::as_str)
            .unwrap_or("home");

        receivers.push_str(&format!(
            "  icmpcheck/{id}:\n    collection_interval: {interval}\n    targets:\n      - host: {}\n        ping_count: 3\n        ping_interval: 1s\n        ping_timeout: 5s\n",
            yaml_quote(address)
        ));

        processors.push_str(&format!(
            "  resource/{id}:\n    attributes:\n      - key: nexus.device.id\n        value: {}\n        action: upsert\n      - key: nexus.device.name\n        value: {}\n        action: upsert\n      - key: nexus.resource.type\n        value: {}\n        action: upsert\n      - key: nexus.site\n        value: {}\n        action: upsert\n      - key: nexus.monitoring.profile\n        value: 'icmp'\n        action: upsert\n  batch/{id}: {{}}\n",
            yaml_quote(id),
            yaml_quote(name),
            yaml_quote(kind),
            yaml_quote(site),
        ));

        pipelines.push_str(&format!(
            "    metrics/{id}:\n      receivers: [icmpcheck/{id}]\n      processors: [resource/{id}, batch/{id}]\n      exporters: [otlp]\n"
        ));
    }

    Ok(format!(
        "{receivers}\n{processors}\nexporters:\n  otlp:\n    endpoint: {}\n    tls:\n      insecure: true\n\nservice:\n  pipelines:\n{pipelines}",
        yaml_quote(endpoint)
    ))
}

async fn command_success(program: &str, args: &[&str]) -> Result<(), Error> {
    let output = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .with_context(|| format!("failed to execute {program}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    bail!(
        "{} failed: {}",
        program,
        if stderr.is_empty() { stdout } else { stderr }
    )
}

async fn reconcile_inventory(inventory: &Value) -> Result<Value, Error> {
    let active_targets = enabled_devices(inventory).len();
    if active_targets == 0 {
        let _ = Command::new("/usr/bin/systemctl")
            .args(["disable", "--now", COLLECTOR_SERVICE])
            .output()
            .await;
        return Ok(json!({
            "active_targets": 0,
            "service": "disabled",
            "config": COLLECTOR_CONFIG_FILENAME
        }));
    }

    if !Path::new(COLLECTOR_BINARY).exists() {
        bail!("OpenTelemetry Collector Contrib is not installed at {COLLECTOR_BINARY}");
    }

    let config = collector_config(inventory)?;
    let temporary = format!("{COLLECTOR_CONFIG_FILENAME}.{}.tmp", std::process::id());
    tokio::fs::write(&temporary, config)
        .await
        .context("unable to write temporary ICMP collector configuration")?;

    let validate_result = command_success(
        COLLECTOR_BINARY,
        &["validate", &format!("--config={temporary}")],
    )
    .await;
    if let Err(err) = validate_result {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(err.context("generated ICMP collector configuration is invalid"));
    }

    tokio::fs::rename(&temporary, COLLECTOR_CONFIG_FILENAME)
        .await
        .context("unable to atomically replace ICMP collector configuration")?;

    command_success("/usr/bin/systemctl", &["daemon-reload"]).await?;
    command_success("/usr/bin/systemctl", &["enable", COLLECTOR_SERVICE]).await?;
    command_success("/usr/bin/systemctl", &["restart", COLLECTOR_SERVICE]).await?;

    Ok(json!({
        "active_targets": active_targets,
        "service": "active",
        "config": COLLECTOR_CONFIG_FILENAME
    }))
}

fn service_active() -> bool {
    std::process::Command::new("/usr/bin/systemctl")
        .args(["is-active", "--quiet", COLLECTOR_SERVICE])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn monitoring_view(inventory: Value) -> Value {
    let enabled = inventory
        .get("devices")
        .and_then(Value::as_array)
        .map(|devices| {
            devices
                .iter()
                .filter(|device| device.get("state").and_then(Value::as_str) == Some("enabled"))
                .count()
        })
        .unwrap_or(0);
    let maintenance = inventory
        .get("devices")
        .and_then(Value::as_array)
        .map(|devices| {
            devices
                .iter()
                .filter(|device| {
                    device.get("state").and_then(Value::as_str) == Some("maintenance")
                })
                .count()
        })
        .unwrap_or(0);

    json!({
        "inventory": inventory,
        "probe_engine": {
            "collector_installed": Path::new(COLLECTOR_BINARY).exists(),
            "service_active": service_active(),
            "active_targets": enabled,
            "maintenance_targets": maintenance,
            "service": COLLECTOR_SERVICE
        }
    })
}

async fn signoz_request(path: &str) -> Result<Value, Error> {
    let base = std::env::var("NEXUS_SIGNOZ_URL").unwrap_or_else(|_| DEFAULT_SIGNOZ_URL.to_string());
    let api_key = std::env::var("NEXUS_SIGNOZ_API_KEY")
        .context("NEXUS_SIGNOZ_API_KEY is not configured")?;
    let base_url = Url::parse(&base).context("invalid NEXUS_SIGNOZ_URL")?;
    if base_url.scheme() != "http" {
        bail!("Nexus SigNoz integration currently requires an internal http URL");
    }
    let host = base_url.host_str().context("SigNoz URL is missing a host")?;
    let port = base_url.port_or_known_default().unwrap_or(80);
    let stream = TcpStream::connect((host, port))
        .await
        .with_context(|| format!("unable to connect to SigNoz at {host}:{port}"))?;
    let io = TokioIo::new(stream);
    let (mut sender, connection) = http1::handshake(io)
        .await
        .context("unable to initialize SigNoz HTTP connection")?;
    tokio::spawn(async move {
        if let Err(err) = connection.await {
            log::debug!("SigNoz HTTP connection closed: {err}");
        }
    });

    let prefix = base_url.path().trim_end_matches('/');
    let uri = format!("{prefix}{path}");
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header("Host", host)
        .header("Accept", "application/json")
        .header("SIGNOZ-API-KEY", api_key)
        .body(Empty::<Bytes>::new())?;
    let response = sender
        .send_request(request)
        .await
        .context("SigNoz API request failed")?;
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .context("unable to read SigNoz API response")?
        .to_bytes();
    if !status.is_success() {
        let detail = String::from_utf8_lossy(&bytes);
        bail!("SigNoz API returned {status}: {detail}");
    }
    serde_json::from_slice(&bytes).context("SigNoz API returned invalid JSON")
}

fn rule_count(value: &Value) -> Option<usize> {
    if let Some(array) = value.as_array() {
        return Some(array.len());
    }
    for key in ["data", "rules"] {
        if let Some(array) = value.get(key).and_then(Value::as_array) {
            return Some(array.len());
        }
    }
    None
}

#[api(
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_AUDIT, false),
    },
)]
/// Return the Nexus monitoring inventory and local ICMP probe-engine state.
pub fn get_monitoring() -> Result<Value, Error> {
    Ok(monitoring_view(read_inventory()?))
}

#[api(
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_AUDIT, false),
    },
    protected: true,
)]
/// Test the configured SigNoz API and return a safe connection summary.
pub async fn signoz_status() -> Result<Value, Error> {
    let url = std::env::var("NEXUS_SIGNOZ_URL").unwrap_or_else(|_| DEFAULT_SIGNOZ_URL.to_string());
    if std::env::var("NEXUS_SIGNOZ_API_KEY").is_err() {
        return Ok(json!({
            "configured": false,
            "connected": false,
            "url": url,
            "error": "NEXUS_SIGNOZ_API_KEY is not configured"
        }));
    }
    match signoz_request("/api/v1/rules").await {
        Ok(rules) => Ok(json!({
            "configured": true,
            "connected": true,
            "url": url,
            "rule_count": rule_count(&rules)
        })),
        Err(err) => Ok(json!({
            "configured": true,
            "connected": false,
            "url": url,
            "error": err.to_string()
        })),
    }
}

#[api(
    input: {
        properties: {
            name: { type: String, description: "Human-readable device name." },
            address: { type: String, description: "Device IPv4, IPv6 or hostname." },
            kind: { type: String, description: "Device category such as switch, access-point, camera or iot." },
            site: { type: String, description: "Nexus site identifier.", optional: true },
            state: { type: String, description: "Monitoring state: enabled, maintenance or disabled.", optional: true },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_MODIFY, false),
    },
    protected: true,
)]
/// Create or update an ICMP-monitored device and reconcile the probe collector.
pub async fn upsert_device(
    name: String,
    address: String,
    kind: String,
    site: Option<String>,
    state: Option<String>,
) -> Result<Value, Error> {
    let name = normalize_name(&name)?;
    let id = normalize_slug(&name)?;
    let address = normalize_address(&address)?;
    let kind = normalize_simple_label(&kind, "device kind")?;
    let site = normalize_simple_label(site.as_deref().unwrap_or("home"), "site")?;
    let state = normalize_state(state.as_deref().unwrap_or("enabled"))?;

    let inventory = {
        let _guard = INVENTORY_WRITE_LOCK
            .lock()
            .map_err(|_| anyhow::anyhow!("monitoring inventory write lock is poisoned"))?;
        let mut inventory = read_inventory()?;
        let devices = inventory
            .get_mut("devices")
            .and_then(Value::as_array_mut)
            .context("monitoring inventory is missing a devices array")?;
        let entry = json!({
            "id": id,
            "name": name,
            "address": address,
            "kind": kind,
            "site": site,
            "profile": "icmp",
            "state": state
        });
        if let Some(existing) = devices
            .iter_mut()
            .find(|device| device.get("id").and_then(Value::as_str) == Some(id.as_str()))
        {
            *existing = entry;
        } else {
            devices.push(entry);
        }
        devices.sort_by(|left, right| {
            left.get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .cmp(right.get("name").and_then(Value::as_str).unwrap_or(""))
        });
        write_inventory(&inventory)?;
        inventory
    };

    let reconcile = reconcile_inventory(&inventory).await;
    Ok(json!({
        "device_id": id,
        "inventory": monitoring_view(inventory),
        "reconcile": match reconcile {
            Ok(value) => value,
            Err(err) => json!({"error": err.to_string()})
        }
    }))
}

#[api(
    input: {
        properties: {
            id: { type: String, description: "Nexus monitoring device id." },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_MODIFY, false),
    },
    protected: true,
)]
/// Delete a monitoring device and reconcile the probe collector.
pub async fn delete_device(id: String) -> Result<Value, Error> {
    let id = normalize_slug(&id)?;
    let inventory = {
        let _guard = INVENTORY_WRITE_LOCK
            .lock()
            .map_err(|_| anyhow::anyhow!("monitoring inventory write lock is poisoned"))?;
        let mut inventory = read_inventory()?;
        let devices = inventory
            .get_mut("devices")
            .and_then(Value::as_array_mut)
            .context("monitoring inventory is missing a devices array")?;
        let before = devices.len();
        devices.retain(|device| device.get("id").and_then(Value::as_str) != Some(id.as_str()));
        if devices.len() == before {
            bail!("monitoring device '{id}' was not found");
        }
        write_inventory(&inventory)?;
        inventory
    };
    let reconcile = reconcile_inventory(&inventory).await;
    Ok(json!({
        "deleted": id,
        "inventory": monitoring_view(inventory),
        "reconcile": match reconcile {
            Ok(value) => value,
            Err(err) => json!({"error": err.to_string()})
        }
    }))
}

#[api(
    input: {
        properties: {
            id: { type: String, description: "Nexus monitoring device id." },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_AUDIT, false),
    },
    protected: true,
)]
/// Run a one-shot ICMP diagnostic for a registered device.
pub async fn probe_device(id: String) -> Result<Value, Error> {
    let id = normalize_slug(&id)?;
    let inventory = read_inventory()?;
    let device = inventory
        .get("devices")
        .and_then(Value::as_array)
        .and_then(|devices| {
            devices
                .iter()
                .find(|device| device.get("id").and_then(Value::as_str) == Some(id.as_str()))
        })
        .context("monitoring device was not found")?;
    let address = device
        .get("address")
        .and_then(Value::as_str)
        .context("monitoring device is missing address")?;

    let output = Command::new("/usr/bin/ping")
        .args(["-n", "-c", "1", "-W", "2", address])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("failed to execute ping")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let latency_ms = stdout
        .split_whitespace()
        .find_map(|part| part.strip_prefix("time="))
        .and_then(|value| value.parse::<f64>().ok());

    Ok(json!({
        "id": id,
        "address": address,
        "reachable": output.status.success(),
        "latency_ms": latency_ms
    }))
}

#[api(
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_MODIFY, false),
    },
    protected: true,
)]
/// Regenerate and apply the ICMP collector configuration from the Nexus inventory.
pub async fn reconcile() -> Result<Value, Error> {
    reconcile_inventory(&read_inventory()?).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_validation_accepts_ip_and_hostname_and_rejects_shell_input() {
        assert_eq!(normalize_address("192.168.0.10").unwrap(), "192.168.0.10");
        assert_eq!(normalize_address("switch-01.internal.").unwrap(), "switch-01.internal");
        assert!(normalize_address("192.168.0.1;id").is_err());
        assert!(normalize_address("-bad.internal").is_err());
    }

    #[test]
    fn state_validation_is_closed() {
        assert_eq!(normalize_state("maintenance").unwrap(), "maintenance");
        assert!(normalize_state("paused").is_err());
    }

    #[test]
    fn generated_config_has_one_isolated_pipeline_per_enabled_device() {
        let inventory = json!({
            "probe": {"otlp_endpoint":"192.168.0.47:4317","collection_interval":"30s"},
            "devices": [
                {"id":"switch-01","name":"Switch 01","address":"192.168.0.5","kind":"switch","site":"home","profile":"icmp","state":"enabled"},
                {"id":"tv-01","name":"TV 01","address":"192.168.0.6","kind":"tv","site":"home","profile":"icmp","state":"maintenance"}
            ]
        });
        let config = collector_config(&inventory).unwrap();
        assert!(config.contains("icmpcheck/switch-01"));
        assert!(config.contains("metrics/switch-01"));
        assert!(config.contains("nexus.device.name"));
        assert!(!config.contains("icmpcheck/tv-01"));
    }
}

use std::process::Stdio;

use anyhow::{Context, Error};
use serde_json::{Value, json};
use tokio::process::Command;

use proxmox_router::{Permission, Router, SubdirMap, list_subdirs_api_method};
use proxmox_schema::api;
use proxmox_sortable_macro::sortable;

use pdm_api_types::{PRIV_SYS_AUDIT, PRIV_SYS_MODIFY};

mod collector;
mod signoz;
mod store;

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
                .filter(|device| device.get("state").and_then(Value::as_str) == Some("maintenance"))
                .count()
        })
        .unwrap_or(0);
    let mut engine = collector::status();
    if let Some(object) = engine.as_object_mut() {
        object.insert("active_targets".to_string(), json!(enabled));
        object.insert("maintenance_targets".to_string(), json!(maintenance));
    }
    json!({
        "inventory": inventory,
        "probe_engine": engine
    })
}

#[api(
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_AUDIT, false),
    },
)]
/// Return the Nexus monitoring inventory and local ICMP probe-engine state.
pub fn get_monitoring() -> Result<Value, Error> {
    Ok(monitoring_view(store::read_inventory()?))
}

#[api(
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_AUDIT, false),
    },
    protected: true,
)]
/// Test the configured SigNoz API and return a safe connection summary.
pub async fn signoz_status() -> Result<Value, Error> {
    Ok(signoz::status().await)
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
    let name = store::normalize_name(&name)?;
    let address = store::normalize_address(&address)?;
    let kind = store::normalize_simple_label(&kind, "device kind")?;
    let site = store::normalize_simple_label(site.as_deref().unwrap_or("home"), "site")?;
    let state = store::normalize_state(state.as_deref().unwrap_or("enabled"))?;
    let (id, inventory) = store::upsert_device(name, address, kind, site, state)?;
    let reconcile = collector::reconcile(&inventory).await;

    Ok(json!({
        "device_id": id,
        "inventory": monitoring_view(inventory),
        "reconcile": match reconcile {
            Ok(value) => value,
            Err(err) => json!({ "error": err.to_string() }),
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
    let id = store::normalize_slug(&id)?;
    let inventory = store::delete_device(&id)?;
    let reconcile = collector::reconcile(&inventory).await;

    Ok(json!({
        "deleted": id,
        "inventory": monitoring_view(inventory),
        "reconcile": match reconcile {
            Ok(value) => value,
            Err(err) => json!({ "error": err.to_string() }),
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
    let id = store::normalize_slug(&id)?;
    let inventory = store::read_inventory()?;
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
    collector::reconcile(&store::read_inventory()?).await
}

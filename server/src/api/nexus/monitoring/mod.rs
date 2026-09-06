use std::process::Stdio;

use anyhow::{Context, Error, bail};
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::sync::Mutex;

use proxmox_router::{Permission, Router, SubdirMap, list_subdirs_api_method};
use proxmox_schema::api;
use proxmox_sortable_macro::sortable;

use pdm_api_types::{PRIV_SYS_AUDIT, PRIV_SYS_MODIFY};

mod collector;
mod signoz;
mod store;

/// Serializes monitoring inventory changes that also synchronize SigNoz maintenance state.
static RESOURCE_LOCK: Mutex<()> = Mutex::const_new(());

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
    ("service", &Router::new().post(&API_METHOD_UPSERT_SERVICE)),
    (
        "service-delete",
        &Router::new().post(&API_METHOD_DELETE_SERVICE)
    ),
    ("signoz", &Router::new().get(&API_METHOD_SIGNOZ_STATUS)),
    (
        "signoz-downtime",
        &Router::new().post(&API_METHOD_CREATE_SIGNOZ_DOWNTIME)
    ),
    (
        "signoz-downtime-delete",
        &Router::new().post(&API_METHOD_DELETE_SIGNOZ_DOWNTIME)
    ),
    (
        "signoz-downtimes",
        &Router::new().get(&API_METHOD_LIST_SIGNOZ_DOWNTIMES)
    ),
    (
        "signoz-rules",
        &Router::new().get(&API_METHOD_LIST_SIGNOZ_RULES)
    ),
]);

pub const ROUTER: Router = Router::new()
    .get(&API_METHOD_GET_MONITORING)
    .subdirs(SUBDIRS);

fn count_state(inventory: &Value, collection: &str, state: &str) -> usize {
    inventory
        .get(collection)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("state").and_then(Value::as_str) == Some(state))
                .count()
        })
        .unwrap_or(0)
}

fn monitoring_view(inventory: Value) -> Value {
    let enabled_devices = count_state(&inventory, "devices", "enabled");
    let maintenance_devices = count_state(&inventory, "devices", "maintenance");
    let enabled_services = count_state(&inventory, "services", "enabled");
    let maintenance_services = count_state(&inventory, "services", "maintenance");
    let mut engine = collector::status();
    if let Some(object) = engine.as_object_mut() {
        object.insert(
            "active_targets".to_string(),
            json!(enabled_devices + enabled_services),
        );
        object.insert("active_devices".to_string(), json!(enabled_devices));
        object.insert("active_services".to_string(), json!(enabled_services));
        object.insert(
            "maintenance_targets".to_string(),
            json!(maintenance_devices + maintenance_services),
        );
        object.insert(
            "maintenance_devices".to_string(),
            json!(maintenance_devices),
        );
        object.insert(
            "maintenance_services".to_string(),
            json!(maintenance_services),
        );
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
/// Return the Nexus monitoring inventory and local collector state.
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
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_AUDIT, false),
    },
    protected: true,
)]
/// Return alert rules from the current SigNoz v2 rules API.
pub async fn list_signoz_rules() -> Result<Value, Error> {
    signoz::list_rules().await
}

#[api(
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_AUDIT, false),
    },
    protected: true,
)]
/// Return planned-maintenance windows from SigNoz.
pub async fn list_signoz_downtimes() -> Result<Value, Error> {
    signoz::list_downtimes().await
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_alert_ids(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

#[api(
    input: {
        properties: {
            name: { type: String, description: "Human-readable planned-maintenance name." },
            description: { type: String, description: "Maintenance reason or operator note.", optional: true },
            timezone: { type: String, description: "IANA timezone, for example America/Monterrey." },
            start_time: { type: String, description: "RFC3339 maintenance start time." },
            end_time: { type: String, description: "RFC3339 maintenance end time." },
            scope: { type: String, description: "Optional SigNoz alert-label scope expression.", optional: true },
            alert_ids: { type: String, description: "Optional comma-separated SigNoz alert rule IDs. Empty applies to all rules matching scope.", optional: true },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_MODIFY, false),
    },
    protected: true,
)]
/// Create a fixed SigNoz planned-maintenance window.
pub async fn create_signoz_downtime(
    name: String,
    timezone: String,
    start_time: String,
    end_time: String,
    description: Option<String>,
    scope: Option<String>,
    alert_ids: Option<String>,
) -> Result<Value, Error> {
    let name = name.trim();
    let timezone = timezone.trim();
    let start_time = start_time.trim();
    let end_time = end_time.trim();
    if name.is_empty() || timezone.is_empty() || start_time.is_empty() || end_time.is_empty() {
        bail!("name, timezone, start_time and end_time are required");
    }
    if name.len() > 160 || timezone.len() > 128 || start_time.len() > 64 || end_time.len() > 64 {
        bail!("planned-maintenance input exceeds the supported length");
    }

    let payload = json!({
        "name": name,
        "description": normalize_optional_text(description).unwrap_or_default(),
        "schedule": {
            "timezone": timezone,
            "startTime": start_time,
            "endTime": end_time
        },
        "alertIds": parse_alert_ids(alert_ids),
        "scope": normalize_optional_text(scope).unwrap_or_default()
    });
    signoz::create_downtime(&payload).await
}

#[api(
    input: {
        properties: {
            id: { type: String, description: "SigNoz planned-maintenance id." },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_MODIFY, false),
    },
    protected: true,
)]
/// Delete a SigNoz planned-maintenance window.
pub async fn delete_signoz_downtime(id: String) -> Result<Value, Error> {
    let id = id.trim();
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        bail!("invalid SigNoz planned-maintenance id");
    }
    signoz::delete_downtime(id).await
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
/// Create or update an ICMP-monitored device and reconcile the collector.
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
    let id = store::normalize_slug(&name)?;

    let _guard = RESOURCE_LOCK.lock().await;

    let previous_inventory = store::read_inventory()?;
    let existing_downtime_id = store::find_device(&previous_inventory, &id)
        .and_then(|device| device.get("signoz_downtime_id").and_then(Value::as_str))
        .map(str::to_string);

    let mut downtime_id = existing_downtime_id.clone();
    let mut signoz_maintenance = None;

    if state == "maintenance" {
        if downtime_id.is_none() {
            match signoz::create_maintenance_downtime(&id, &name).await {
                Ok(created_id) => {
                    signoz_maintenance = Some(json!({ "downtime_created": created_id }));
                    downtime_id = Some(created_id);
                }
                Err(err) => {
                    signoz_maintenance = Some(json!({ "downtime_error": err.to_string() }));
                }
            }
        }
    } else if let Some(existing_id) = existing_downtime_id {
        match signoz::delete_downtime(&existing_id).await {
            Ok(_) => {
                signoz_maintenance = Some(json!({ "downtime_deleted": existing_id }));
                downtime_id = None;
            }
            Err(err) => {
                signoz_maintenance = Some(json!({ "downtime_error": err.to_string() }));
            }
        }
    }

    let (id, inventory) = store::upsert_device(name, address, kind, site, state, downtime_id)?;
    let reconcile = collector::reconcile(&inventory).await;

    Ok(json!({
        "device_id": id,
        "inventory": monitoring_view(inventory),
        "reconcile": match reconcile {
            Ok(value) => value,
            Err(err) => json!({ "error": err.to_string() }),
        },
        "signoz_maintenance": signoz_maintenance,
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
/// Delete a monitoring device and reconcile the collector.
pub async fn delete_device(id: String) -> Result<Value, Error> {
    let id = store::normalize_slug(&id)?;
    let _guard = RESOURCE_LOCK.lock().await;
    let previous_inventory = store::read_inventory()?;
    let downtime_id = store::find_device(&previous_inventory, &id)
        .and_then(|device| device.get("signoz_downtime_id").and_then(Value::as_str))
        .map(str::to_string);

    let inventory = store::delete_device(&id)?;
    if let Some(downtime_id) = downtime_id {
        if let Err(err) = signoz::delete_downtime(&downtime_id).await {
            log::warn!(
                "unable to remove SigNoz downtime {downtime_id} for deleted device {id}: {err}"
            );
        }
    }
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
            name: { type: String, description: "Human-readable service name." },
            address: { type: String, description: "Service IPv4, IPv6 or hostname." },
            port: { type: u16, description: "Prometheus metrics TCP port." },
            metrics_path: { type: String, description: "Prometheus metrics path, normally /metrics.", optional: true },
            site: { type: String, description: "Nexus site identifier.", optional: true },
            state: { type: String, description: "Monitoring state: enabled, maintenance or disabled.", optional: true },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_MODIFY, false),
    },
    protected: true,
)]
/// Create or update a Prometheus-monitored service and reconcile the collector.
pub async fn upsert_service(
    name: String,
    address: String,
    port: u16,
    metrics_path: Option<String>,
    site: Option<String>,
    state: Option<String>,
) -> Result<Value, Error> {
    if port == 0 {
        bail!("service port must be between 1 and 65535");
    }
    let name = store::normalize_name(&name)?;
    let address = store::normalize_address(&address)?;
    let metrics_path =
        store::normalize_metrics_path(metrics_path.as_deref().unwrap_or("/metrics"))?;
    let site = store::normalize_simple_label(site.as_deref().unwrap_or("home"), "site")?;
    let state = store::normalize_state(state.as_deref().unwrap_or("enabled"))?;
    let id = store::normalize_slug(&name)?;

    let _guard = RESOURCE_LOCK.lock().await;
    let previous_inventory = store::read_inventory()?;
    let existing_downtime_id = store::find_service(&previous_inventory, &id)
        .and_then(|service| service.get("signoz_downtime_id").and_then(Value::as_str))
        .map(str::to_string);
    let mut downtime_id = existing_downtime_id.clone();
    let mut signoz_maintenance = None;

    if state == "maintenance" {
        if downtime_id.is_none() {
            match signoz::create_service_maintenance_downtime(&id, &name).await {
                Ok(created_id) => {
                    signoz_maintenance = Some(json!({ "downtime_created": created_id }));
                    downtime_id = Some(created_id);
                }
                Err(err) => {
                    signoz_maintenance = Some(json!({ "downtime_error": err.to_string() }));
                }
            }
        }
    } else if let Some(existing_id) = existing_downtime_id {
        match signoz::delete_downtime(&existing_id).await {
            Ok(_) => {
                signoz_maintenance = Some(json!({ "downtime_deleted": existing_id }));
                downtime_id = None;
            }
            Err(err) => {
                signoz_maintenance = Some(json!({ "downtime_error": err.to_string() }));
            }
        }
    }

    let (id, inventory) =
        store::upsert_service(name, address, port, metrics_path, site, state, downtime_id)?;
    let reconcile = collector::reconcile(&inventory).await;

    Ok(json!({
        "service_id": id,
        "inventory": monitoring_view(inventory),
        "reconcile": match reconcile {
            Ok(value) => value,
            Err(err) => json!({ "error": err.to_string() }),
        },
        "signoz_maintenance": signoz_maintenance,
    }))
}

#[api(
    input: {
        properties: {
            id: { type: String, description: "Nexus monitoring service id." },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_MODIFY, false),
    },
    protected: true,
)]
/// Delete a monitoring service and reconcile the collector.
pub async fn delete_service(id: String) -> Result<Value, Error> {
    let id = store::normalize_slug(&id)?;
    let _guard = RESOURCE_LOCK.lock().await;
    let previous_inventory = store::read_inventory()?;
    let downtime_id = store::find_service(&previous_inventory, &id)
        .and_then(|service| service.get("signoz_downtime_id").and_then(Value::as_str))
        .map(str::to_string);
    let inventory = store::delete_service(&id)?;
    if let Some(downtime_id) = downtime_id {
        if let Err(err) = signoz::delete_downtime(&downtime_id).await {
            log::warn!(
                "unable to remove SigNoz downtime {downtime_id} for deleted service {id}: {err}"
            );
        }
    }
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
/// Regenerate and apply the monitoring collector configuration from the Nexus inventory.
pub async fn reconcile() -> Result<Value, Error> {
    collector::reconcile(&store::read_inventory()?).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_ids_are_trimmed_and_empty_values_removed() {
        assert_eq!(
            parse_alert_ids(Some("rule-a, rule-b,,".to_string())),
            vec!["rule-a".to_string(), "rule-b".to_string()]
        );
    }

    #[test]
    fn monitoring_view_counts_devices_and_services() {
        let view = monitoring_view(json!({
            "devices": [{"state": "enabled"}, {"state": "maintenance"}],
            "services": [{"state": "enabled"}, {"state": "enabled"}]
        }));
        assert_eq!(
            view.pointer("/probe_engine/active_devices"),
            Some(&json!(1))
        );
        assert_eq!(
            view.pointer("/probe_engine/active_services"),
            Some(&json!(2))
        );
        assert_eq!(
            view.pointer("/probe_engine/active_targets"),
            Some(&json!(3))
        );
    }
}

use std::net::IpAddr;
use std::sync::Mutex;

use anyhow::{Context, Error, bail};
use serde_json::{Value, json};

use pdm_buildcfg::configdir;

pub(super) const INVENTORY_FILENAME: &str = configdir!("/monitoring.json");
pub(super) const DEFAULT_SIGNOZ_URL: &str = "http://192.168.0.47:8080";
pub(super) const DEFAULT_OTLP_ENDPOINT: &str = "192.168.0.47:4317";

static INVENTORY_WRITE_LOCK: Mutex<()> = Mutex::new(());

pub(super) fn default_inventory() -> Value {
    json!({
        "version": 1,
        "signoz": { "url": DEFAULT_SIGNOZ_URL },
        "probe": {
            "otlp_endpoint": DEFAULT_OTLP_ENDPOINT,
            "collection_interval": "30s"
        },
        "devices": []
    })
}

pub(super) fn read_inventory() -> Result<Value, Error> {
    let Some(raw) = proxmox_sys::fs::file_read_optional_string(INVENTORY_FILENAME)? else {
        return Ok(default_inventory());
    };
    serde_json::from_str(&raw).context("unable to parse monitoring.json")
}

fn write_inventory(inventory: &Value) -> Result<(), Error> {
    let temporary = format!("{INVENTORY_FILENAME}.{}.tmp", std::process::id());
    let contents = serde_json::to_vec_pretty(inventory)?;
    std::fs::write(&temporary, contents)
        .context("unable to write temporary monitoring inventory")?;
    std::fs::rename(&temporary, INVENTORY_FILENAME)
        .context("unable to atomically replace monitoring inventory")?;
    Ok(())
}

pub(super) fn normalize_name(input: &str) -> Result<String, Error> {
    let name = input.trim();
    if name.is_empty() || name.len() > 80 {
        bail!("device name must contain between 1 and 80 characters");
    }
    if name.chars().any(char::is_control) {
        bail!("device name contains invalid control characters");
    }
    Ok(name.to_string())
}

pub(super) fn normalize_slug(input: &str) -> Result<String, Error> {
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

pub(super) fn normalize_simple_label(input: &str, field: &str) -> Result<String, Error> {
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

pub(super) fn normalize_address(input: &str) -> Result<String, Error> {
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

pub(super) fn normalize_state(input: &str) -> Result<String, Error> {
    match input.trim().to_ascii_lowercase().as_str() {
        "enabled" => Ok("enabled".to_string()),
        "maintenance" => Ok("maintenance".to_string()),
        "disabled" => Ok("disabled".to_string()),
        _ => bail!("state must be enabled, maintenance or disabled"),
    }
}

pub(super) fn upsert_device(
    name: String,
    address: String,
    kind: String,
    site: String,
    state: String,
) -> Result<(String, Value), Error> {
    let id = normalize_slug(&name)?;
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
    Ok((id, inventory))
}

pub(super) fn delete_device(id: &str) -> Result<Value, Error> {
    let _guard = INVENTORY_WRITE_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("monitoring inventory write lock is poisoned"))?;
    let mut inventory = read_inventory()?;
    let devices = inventory
        .get_mut("devices")
        .and_then(Value::as_array_mut)
        .context("monitoring inventory is missing a devices array")?;
    let before = devices.len();
    devices.retain(|device| device.get("id").and_then(Value::as_str) != Some(id));
    if devices.len() == before {
        bail!("monitoring device '{id}' was not found");
    }
    write_inventory(&inventory)?;
    Ok(inventory)
}

pub(super) fn enabled_devices(inventory: &Value) -> Vec<&Value> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_validation_accepts_ip_and_hostname_and_rejects_shell_input() {
        assert_eq!(normalize_address("192.168.0.10").unwrap(), "192.168.0.10");
        assert_eq!(
            normalize_address("switch-01.internal.").unwrap(),
            "switch-01.internal"
        );
        assert!(normalize_address("192.168.0.1;id").is_err());
        assert!(normalize_address("-bad.internal").is_err());
    }

    #[test]
    fn state_validation_is_closed() {
        assert_eq!(normalize_state("maintenance").unwrap(), "maintenance");
        assert!(normalize_state("paused").is_err());
    }
}

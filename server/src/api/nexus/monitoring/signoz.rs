use anyhow::{Context, Error, bail};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use url::Url;

use super::store::DEFAULT_SIGNOZ_URL;

const RULES_PATH: &str = "/api/v2/rules";
const DOWNTIME_PATH: &str = "/api/v1/downtime_schedules";

async fn request(method: Method, path: &str, body: Option<&Value>) -> Result<Value, Error> {
    let base = std::env::var("NEXUS_SIGNOZ_URL").unwrap_or_else(|_| DEFAULT_SIGNOZ_URL.to_string());
    let api_key =
        std::env::var("NEXUS_SIGNOZ_API_KEY").context("NEXUS_SIGNOZ_API_KEY is not configured")?;
    let base_url = Url::parse(&base).context("invalid NEXUS_SIGNOZ_URL")?;
    if base_url.scheme() != "http" {
        bail!("Nexus SigNoz integration currently requires an internal http URL");
    }
    let host = base_url
        .host_str()
        .context("SigNoz URL is missing a host")?;
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
    let body = match body {
        Some(value) => Full::new(Bytes::from(serde_json::to_vec(value)?)),
        None => Full::new(Bytes::new()),
    };
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("Host", host)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("SIGNOZ-API-KEY", api_key)
        .body(body)?;
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
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&bytes).context("SigNoz API returned invalid JSON")
}

fn count_collection(value: &Value, keys: &[&str]) -> Option<usize> {
    if let Some(array) = value.as_array() {
        return Some(array.len());
    }
    for key in keys {
        if let Some(array) = value.get(*key).and_then(Value::as_array) {
            return Some(array.len());
        }
    }
    None
}

pub(super) async fn list_rules() -> Result<Value, Error> {
    request(Method::GET, RULES_PATH, None).await
}

pub(super) async fn list_downtimes() -> Result<Value, Error> {
    request(Method::GET, DOWNTIME_PATH, None).await
}

pub(super) async fn create_downtime(payload: &Value) -> Result<Value, Error> {
    request(Method::POST, DOWNTIME_PATH, Some(payload)).await
}

pub(super) async fn delete_downtime(id: &str) -> Result<Value, Error> {
    request(Method::DELETE, &format!("{DOWNTIME_PATH}/{id}"), None).await
}

/// Nexus device identity in SigNoz is the `nexus_device_id` label attached to every
/// metric the ICMP probe pipeline emits for that device (see `collector::collector_config`).
/// A downtime scoped to `nexus_device_id="<id>"` therefore silences alerts for exactly
/// that device and no other resource.
fn device_scope(device_id: &str) -> String {
    format!("nexus_device_id=\"{device_id}\"")
}

fn extract_downtime_id(response: &Value) -> Result<String, Error> {
    for pointer in [
        "/id",
        "/data/id",
        "/data/downtimeSchedule/id",
        "/downtimeSchedule/id",
    ] {
        if let Some(id) = response.pointer(pointer).and_then(Value::as_str) {
            return Ok(id.to_string());
        }
        if let Some(id) = response.pointer(pointer).and_then(Value::as_u64) {
            return Ok(id.to_string());
        }
    }
    bail!("SigNoz did not return a recognizable downtime id: {response}");
}

/// Create (or replace) the planned-maintenance window that suppresses alerts for a
/// single Nexus-managed device while it is in the local `maintenance` state. The
/// window is open-ended on the SigNoz side (a far-future end time); Nexus deletes it
/// explicitly via [`delete_downtime`] as soon as the device leaves maintenance, so the
/// long end time is only a safety bound against an orphaned schedule.
pub(super) async fn create_maintenance_downtime(
    device_id: &str,
    device_name: &str,
) -> Result<String, Error> {
    let start = proxmox_time::epoch_i64();
    let end = start + 10 * 365 * 24 * 60 * 60;
    let payload = json!({
        "name": format!("Nexus maintenance: {device_name}"),
        "description": format!(
            "Automatically created by Nexus while device '{device_id}' is in Maintenance."
        ),
        "schedule": {
            "timezone": "UTC",
            "startTime": proxmox_time::epoch_to_rfc3339_utc(start)?,
            "endTime": proxmox_time::epoch_to_rfc3339_utc(end)?,
        },
        "alertIds": Vec::<String>::new(),
        "scope": device_scope(device_id),
    });
    let response = create_downtime(&payload).await?;
    extract_downtime_id(&response)
}

pub(super) async fn status() -> Value {
    let url = std::env::var("NEXUS_SIGNOZ_URL").unwrap_or_else(|_| DEFAULT_SIGNOZ_URL.to_string());
    if std::env::var("NEXUS_SIGNOZ_API_KEY").is_err() {
        return json!({
            "configured": false,
            "connected": false,
            "url": url,
            "error": "NEXUS_SIGNOZ_API_KEY is not configured"
        });
    }

    match list_rules().await {
        Ok(rules) => {
            let downtimes = list_downtimes().await;
            json!({
                "configured": true,
                "connected": true,
                "url": url,
                "rules_api": RULES_PATH,
                "rule_count": count_collection(&rules, &["data", "rules"]),
                "downtime_count": downtimes
                    .as_ref()
                    .ok()
                    .and_then(|value| count_collection(value, &["data", "downtime_schedules", "downtimeSchedules"])),
                "downtime_api_connected": downtimes.is_ok(),
                "downtime_error": downtimes.err().map(|err| err.to_string())
            })
        }
        Err(err) => json!({
            "configured": true,
            "connected": false,
            "url": url,
            "rules_api": RULES_PATH,
            "error": err.to_string()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_count_supports_current_and_wrapped_shapes() {
        assert_eq!(count_collection(&json!([1, 2]), &["data"]), Some(2));
        assert_eq!(
            count_collection(&json!({"data": [1, 2, 3]}), &["data"]),
            Some(3)
        );
        assert_eq!(count_collection(&json!({"rules": []}), &["rules"]), Some(0));
        assert_eq!(count_collection(&json!({"data": {}}), &["data"]), None);
    }

    #[test]
    fn device_scope_matches_the_confirmed_signoz_label() {
        assert_eq!(
            device_scope("zigbee-slzb-06m"),
            "nexus_device_id=\"zigbee-slzb-06m\""
        );
    }

    #[test]
    fn downtime_id_extraction_supports_current_and_wrapped_shapes() {
        assert_eq!(extract_downtime_id(&json!({"id": "dt-1"})).unwrap(), "dt-1");
        assert_eq!(
            extract_downtime_id(&json!({"data": {"id": "dt-2"}})).unwrap(),
            "dt-2"
        );
        assert_eq!(
            extract_downtime_id(&json!({"data": {"downtimeSchedule": {"id": 42}}})).unwrap(),
            "42"
        );
        assert!(extract_downtime_id(&json!({"status": "ok"})).is_err());
    }
}

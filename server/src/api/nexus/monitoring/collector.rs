use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Error, bail};
use serde_json::{Value, json};
use tokio::process::Command;

use pdm_buildcfg::configdir;

use super::store::{DEFAULT_OTLP_ENDPOINT, enabled_devices};

const COLLECTOR_CONFIG_FILENAME: &str = configdir!("/nexus-icmp-collector.yaml");
const COLLECTOR_BINARY: &str = "/usr/bin/otelcol-contrib";
const COLLECTOR_SERVICE: &str = "nexus-icmp-collector.service";
const BLACKBOX_BINARY: &str = "/usr/bin/prometheus-blackbox-exporter";
const BLACKBOX_SERVICE: &str = "nexus-blackbox-exporter.service";
const BLACKBOX_ENDPOINT: &str = "127.0.0.1:9116";

fn yaml_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
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
        let site = device.get("site").and_then(Value::as_str).unwrap_or("home");

        receivers.push_str(&format!(
            "  prometheus/{id}:\n    config:\n      scrape_configs:\n        - job_name: {}\n          scrape_interval: {interval}\n          scrape_timeout: 10s\n          metrics_path: /probe\n          params:\n            module: [icmp]\n          static_configs:\n            - targets:\n                - {}\n          relabel_configs:\n            - source_labels: [__address__]\n              target_label: __param_target\n            - source_labels: [__param_target]\n              target_label: instance\n            - target_label: __address__\n              replacement: {}\n",
            yaml_quote(&format!("nexus-icmp-{id}")),
            yaml_quote(address),
            yaml_quote(BLACKBOX_ENDPOINT),
        ));
        processors.push_str(&format!(
            "  resource/{id}:\n    attributes:\n      - key: nexus.device.id\n        value: {}\n        action: upsert\n      - key: nexus.device.name\n        value: {}\n        action: upsert\n      - key: nexus.resource.type\n        value: {}\n        action: upsert\n      - key: nexus.site\n        value: {}\n        action: upsert\n      - key: nexus.monitoring.profile\n        value: 'icmp'\n        action: upsert\n  batch/{id}: {{}}\n",
            yaml_quote(id),
            yaml_quote(name),
            yaml_quote(kind),
            yaml_quote(site),
        ));
        pipelines.push_str(&format!(
            "    metrics/{id}:\n      receivers: [prometheus/{id}]\n      processors: [resource/{id}, batch/{id}]\n      exporters: [otlp]\n"
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

async fn disable_service(service: &str) {
    let _ = Command::new("/usr/bin/systemctl")
        .args(["disable", "--now", service])
        .output()
        .await;
}

pub(super) async fn reconcile(inventory: &Value) -> Result<Value, Error> {
    let active_targets = enabled_devices(inventory).len();
    if active_targets == 0 {
        disable_service(COLLECTOR_SERVICE).await;
        disable_service(BLACKBOX_SERVICE).await;
        let _ = tokio::fs::remove_file(COLLECTOR_CONFIG_FILENAME).await;
        return Ok(json!({
            "active_targets": 0,
            "service": "disabled",
            "prober_service": "disabled",
            "config": COLLECTOR_CONFIG_FILENAME
        }));
    }

    if !Path::new(COLLECTOR_BINARY).exists() {
        bail!("OpenTelemetry Collector Contrib is not installed at {COLLECTOR_BINARY}");
    }
    if !Path::new(BLACKBOX_BINARY).exists() {
        bail!("Prometheus Blackbox Exporter is not installed at {BLACKBOX_BINARY}");
    }

    let config = collector_config(inventory)?;
    let temporary = format!("{COLLECTOR_CONFIG_FILENAME}.{}.tmp", std::process::id());
    tokio::fs::write(&temporary, config)
        .await
        .context("unable to write temporary ICMP collector configuration")?;

    let config_arg = format!("--config={temporary}");
    if let Err(err) = command_success(COLLECTOR_BINARY, &["validate", &config_arg]).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        bail!("generated ICMP collector configuration is invalid: {err:#}");
    }

    tokio::fs::rename(&temporary, COLLECTOR_CONFIG_FILENAME)
        .await
        .context("unable to atomically replace ICMP collector configuration")?;
    command_success("/usr/bin/systemctl", &["daemon-reload"]).await?;
    command_success("/usr/bin/systemctl", &["enable", BLACKBOX_SERVICE]).await?;
    command_success("/usr/bin/systemctl", &["restart", BLACKBOX_SERVICE]).await?;
    command_success("/usr/bin/systemctl", &["enable", COLLECTOR_SERVICE]).await?;
    command_success("/usr/bin/systemctl", &["restart", COLLECTOR_SERVICE]).await?;

    Ok(json!({
        "active_targets": active_targets,
        "service": "active",
        "prober_service": "active",
        "config": COLLECTOR_CONFIG_FILENAME
    }))
}

fn service_active(service: &str) -> bool {
    std::process::Command::new("/usr/bin/systemctl")
        .args(["is-active", "--quiet", service])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(super) fn status() -> Value {
    json!({
        "collector_installed": Path::new(COLLECTOR_BINARY).exists(),
        "prober_installed": Path::new(BLACKBOX_BINARY).exists(),
        "service_active": service_active(COLLECTOR_SERVICE),
        "prober_service_active": service_active(BLACKBOX_SERVICE),
        "service": COLLECTOR_SERVICE,
        "prober_service": BLACKBOX_SERVICE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_config_has_one_isolated_pipeline_per_enabled_device() {
        let inventory = json!({
            "probe": {
                "otlp_endpoint": "192.168.0.47:4317",
                "collection_interval": "30s"
            },
            "devices": [
                {
                    "id": "switch-01",
                    "name": "Switch 01",
                    "address": "192.168.0.5",
                    "kind": "switch",
                    "site": "home",
                    "profile": "icmp",
                    "state": "enabled"
                },
                {
                    "id": "tv-01",
                    "name": "TV 01",
                    "address": "192.168.0.6",
                    "kind": "tv",
                    "site": "home",
                    "profile": "icmp",
                    "state": "maintenance"
                }
            ]
        });
        let config = collector_config(&inventory).unwrap();
        assert!(config.contains("prometheus/switch-01"));
        assert!(config.contains("module: [icmp]"));
        assert!(config.contains("127.0.0.1:9116"));
        assert!(config.contains("metrics/switch-01"));
        assert!(config.contains("nexus.device.name"));
        assert!(!config.contains("prometheus/tv-01"));
    }
}

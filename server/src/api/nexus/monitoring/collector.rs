use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Error, bail};
use serde_json::{Value, json};
use tokio::process::Command;

use pdm_buildcfg::configdir;

use super::store::{DEFAULT_OTLP_ENDPOINT, enabled_devices, enabled_services};

const COLLECTOR_CONFIG_FILENAME: &str = configdir!("/nexus-icmp-collector.yaml");
const COLLECTOR_BINARY: &str = "/usr/bin/otelcol-contrib";
const COLLECTOR_SERVICE: &str = "nexus-icmp-collector.service";
const BLACKBOX_BINARY: &str = "/usr/bin/prometheus-blackbox-exporter";
const BLACKBOX_CONFIG: &str = "/etc/prometheus/blackbox.yml";
const BLACKBOX_SERVICE: &str = "prometheus-blackbox-exporter.service";
const BLACKBOX_ENDPOINT: &str = "127.0.0.1:9115";
const COLLECTOR_TELEMETRY_PORT: u16 = 8889;

fn yaml_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn collector_config(inventory: &Value) -> Result<String, Error> {
    let devices = enabled_devices(inventory);
    let services = enabled_services(inventory);
    if devices.is_empty() && services.is_empty() {
        bail!("no enabled monitoring targets");
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
    let mut pipelines = String::new();

    if !devices.is_empty() {
        let mut static_configs = String::new();
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

            static_configs.push_str(&format!(
                "            - targets: [{}]\n              labels:\n                nexus_device_id: {}\n                nexus_device_name: {}\n                nexus_resource_type: {}\n                nexus_site: {}\n                nexus_monitoring_profile: 'icmp'\n",
                yaml_quote(address),
                yaml_quote(id),
                yaml_quote(name),
                yaml_quote(kind),
                yaml_quote(site),
            ));
        }

        receivers.push_str(&format!(
            "  prometheus/nexus_icmp:\n    config:\n      scrape_configs:\n        - job_name: 'nexus_icmp'\n          scrape_interval: {interval}\n          metrics_path: /probe\n          params:\n            module: ['icmp']\n          static_configs:\n{static_configs}          relabel_configs:\n            - source_labels: [__address__]\n              target_label: __param_target\n            - source_labels: [__param_target]\n              target_label: instance\n            - target_label: __address__\n              replacement: '{BLACKBOX_ENDPOINT}'\n"
        ));
        pipelines.push_str(
            "    metrics/nexus_icmp:\n      receivers: [prometheus/nexus_icmp]\n      processors: [batch]\n      exporters: [otlp]\n",
        );
    }

    for service in services {
        let id = service
            .get("id")
            .and_then(Value::as_str)
            .context("monitoring service is missing id")?;
        let name = service
            .get("name")
            .and_then(Value::as_str)
            .context("monitoring service is missing name")?;
        let address = service
            .get("address")
            .and_then(Value::as_str)
            .context("monitoring service is missing address")?;
        let port = service
            .get("port")
            .and_then(Value::as_u64)
            .context("monitoring service is missing port")?;
        let metrics_path = service
            .get("metrics_path")
            .and_then(Value::as_str)
            .unwrap_or("/metrics");
        let site = service.get("site").and_then(Value::as_str).unwrap_or("home");
        let receiver = format!("prometheus/nexus_service_{id}");
        let target = format!("{address}:{port}");

        receivers.push_str(&format!(
            "  {receiver}:\n    config:\n      scrape_configs:\n        - job_name: {}\n          scrape_interval: {interval}\n          metrics_path: {}\n          static_configs:\n            - targets: [{}]\n              labels:\n                nexus_service_id: {}\n                nexus_service_name: {}\n                nexus_resource_type: 'service'\n                nexus_site: {}\n                nexus_monitoring_profile: 'prometheus'\n",
            yaml_quote(&format!("nexus_service_{id}")),
            yaml_quote(metrics_path),
            yaml_quote(&target),
            yaml_quote(id),
            yaml_quote(name),
            yaml_quote(site),
        ));
        pipelines.push_str(&format!(
            "    metrics/nexus_service_{id}:\n      receivers: [{receiver}]\n      processors: [batch]\n      exporters: [otlp]\n"
        ));
    }

    Ok(format!(
        "{receivers}\nprocessors:\n  batch: {{}}\n\nexporters:\n  otlp:\n    endpoint: {}\n    tls:\n      insecure: true\n\nservice:\n  telemetry:\n    metrics:\n      readers:\n        - pull:\n            exporter:\n              prometheus:\n                host: '127.0.0.1'\n                port: {COLLECTOR_TELEMETRY_PORT}\n  pipelines:\n{pipelines}",
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

pub(super) async fn reconcile(inventory: &Value) -> Result<Value, Error> {
    let active_devices = enabled_devices(inventory).len();
    let active_services = enabled_services(inventory).len();
    let active_targets = active_devices + active_services;
    if active_targets == 0 {
        let _ = Command::new("/usr/bin/systemctl")
            .args(["disable", "--now", COLLECTOR_SERVICE])
            .output()
            .await;
        let _ = Command::new("/usr/bin/systemctl")
            .args(["disable", "--now", BLACKBOX_SERVICE])
            .output()
            .await;
        return Ok(json!({
            "active_targets": 0,
            "active_devices": 0,
            "active_services": 0,
            "service": "disabled",
            "config": COLLECTOR_CONFIG_FILENAME
        }));
    }

    if !Path::new(COLLECTOR_BINARY).exists() {
        bail!("OpenTelemetry Collector Contrib is not installed at {COLLECTOR_BINARY}");
    }
    if active_devices > 0 {
        if !Path::new(BLACKBOX_BINARY).exists() {
            bail!("Prometheus Blackbox Exporter is not installed at {BLACKBOX_BINARY}");
        }
        if !Path::new(BLACKBOX_CONFIG).exists() {
            bail!("Prometheus Blackbox Exporter configuration is missing at {BLACKBOX_CONFIG}");
        }
        command_success(
            BLACKBOX_BINARY,
            &[
                "--config.file=/etc/prometheus/blackbox.yml",
                "--config.check",
            ],
        )
        .await
        .context("Prometheus Blackbox Exporter configuration is invalid")?;
    }

    let config = collector_config(inventory)?;
    let temporary = format!("{COLLECTOR_CONFIG_FILENAME}.{}.tmp", std::process::id());
    tokio::fs::write(&temporary, config)
        .await
        .context("unable to write temporary monitoring collector configuration")?;

    let config_arg = format!("--config={temporary}");
    if let Err(err) = command_success(COLLECTOR_BINARY, &["validate", &config_arg]).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(err.context("generated monitoring collector configuration is invalid"));
    }

    tokio::fs::rename(&temporary, COLLECTOR_CONFIG_FILENAME)
        .await
        .context("unable to atomically replace monitoring collector configuration")?;
    command_success("/usr/bin/systemctl", &["daemon-reload"]).await?;
    if active_devices > 0 {
        command_success("/usr/bin/systemctl", &["enable", "--now", BLACKBOX_SERVICE]).await?;
    } else {
        let _ = Command::new("/usr/bin/systemctl")
            .args(["disable", "--now", BLACKBOX_SERVICE])
            .output()
            .await;
    }
    command_success("/usr/bin/systemctl", &["enable", COLLECTOR_SERVICE]).await?;
    command_success("/usr/bin/systemctl", &["restart", COLLECTOR_SERVICE]).await?;

    Ok(json!({
        "active_targets": active_targets,
        "active_devices": active_devices,
        "active_services": active_services,
        "service": "active",
        "config": COLLECTOR_CONFIG_FILENAME
    }))
}

pub(super) fn status() -> Value {
    let service_active = std::process::Command::new("/usr/bin/systemctl")
        .args(["is-active", "--quiet", COLLECTOR_SERVICE])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    let blackbox_active = std::process::Command::new("/usr/bin/systemctl")
        .args(["is-active", "--quiet", BLACKBOX_SERVICE])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    json!({
        "collector_installed": Path::new(COLLECTOR_BINARY).exists(),
        "blackbox_installed": Path::new(BLACKBOX_BINARY).exists(),
        "blackbox_active": blackbox_active,
        "service_active": service_active,
        "service": COLLECTOR_SERVICE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_config_uses_prometheus_blackbox_for_enabled_devices() {
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
            ],
            "services": []
        });
        let config = collector_config(&inventory).unwrap();
        assert!(config.contains("prometheus/nexus_icmp"));
        assert!(config.contains("module: ['icmp']"));
        assert!(config.contains("192.168.0.5"));
        assert!(config.contains("nexus_device_name"));
        assert!(config.contains(BLACKBOX_ENDPOINT));
        assert!(config.contains("host: '127.0.0.1'"));
        assert!(config.contains(&format!("port: {COLLECTOR_TELEMETRY_PORT}")));
        assert!(!config.contains("192.168.0.6"));
        assert!(!config.contains("icmpcheck/"));
    }

    #[test]
    fn generated_config_scrapes_enabled_prometheus_services() {
        let inventory = json!({
            "probe": {
                "otlp_endpoint": "192.168.0.47:4317",
                "collection_interval": "30s"
            },
            "devices": [],
            "services": [
                {
                    "id": "dreamteclabs-notify",
                    "name": "DreamtecLabs Notify",
                    "address": "192.168.0.40",
                    "port": 8000,
                    "metrics_path": "/metrics",
                    "site": "home",
                    "profile": "prometheus",
                    "state": "enabled"
                },
                {
                    "id": "paused",
                    "name": "Paused",
                    "address": "192.168.0.41",
                    "port": 9000,
                    "metrics_path": "/metrics",
                    "site": "home",
                    "profile": "prometheus",
                    "state": "maintenance"
                }
            ]
        });
        let config = collector_config(&inventory).unwrap();
        assert!(config.contains("prometheus/nexus_service_dreamteclabs-notify"));
        assert!(config.contains("192.168.0.40:8000"));
        assert!(config.contains("metrics_path: '/metrics'"));
        assert!(config.contains("nexus_service_id: 'dreamteclabs-notify'"));
        assert!(config.contains("nexus_monitoring_profile: 'prometheus'"));
        assert!(!config.contains("192.168.0.41:9000"));
        assert!(!config.contains("prometheus/nexus_icmp"));
    }
}

#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

grep -q 'pub mod monitoring;' server/src/api/nexus/mod.rs
grep -q 'nexus::monitoring::ROUTER' server/src/api/mod.rs
grep -q 'API_METHOD_SIGNOZ_STATUS' server/src/api/nexus/monitoring/mod.rs
grep -q 'API_METHOD_LIST_SIGNOZ_RULES' server/src/api/nexus/monitoring/mod.rs
grep -q 'API_METHOD_LIST_SIGNOZ_DOWNTIMES' server/src/api/nexus/monitoring/mod.rs
grep -q 'API_METHOD_CREATE_SIGNOZ_DOWNTIME' server/src/api/nexus/monitoring/mod.rs
grep -q 'API_METHOD_DELETE_SIGNOZ_DOWNTIME' server/src/api/nexus/monitoring/mod.rs
grep -q 'NEXUS_SIGNOZ_API_KEY' server/src/api/nexus/monitoring/signoz.rs
grep -q 'SIGNOZ-API-KEY' server/src/api/nexus/monitoring/signoz.rs
grep -q 'const RULES_PATH: &str = "/api/v2/rules"' server/src/api/nexus/monitoring/signoz.rs
grep -q 'const DOWNTIME_PATH: &str = "/api/v1/downtime_schedules"' server/src/api/nexus/monitoring/signoz.rs
grep -q 'Method::POST, DOWNTIME_PATH' server/src/api/nexus/monitoring/signoz.rs
grep -q 'Method::DELETE' server/src/api/nexus/monitoring/signoz.rs
grep -q 'prometheus/nexus_icmp' server/src/api/nexus/monitoring/collector.rs
grep -q 'prometheus-blackbox-exporter' server/src/api/nexus/monitoring/collector.rs
grep -q 'nexus_device_id' server/src/api/nexus/monitoring/collector.rs
grep -q 'COLLECTOR_TELEMETRY_PORT: u16 = 8889' server/src/api/nexus/monitoring/collector.rs
grep -q "host: '127.0.0.1'" server/src/api/nexus/monitoring/collector.rs
grep -q 'maintenance' server/src/api/nexus/monitoring/store.rs
grep -q 'fn create_maintenance_downtime' server/src/api/nexus/monitoring/signoz.rs
grep -q 'nexus_device_id=' server/src/api/nexus/monitoring/signoz.rs
grep -q 'signoz_downtime_id' server/src/api/nexus/monitoring/store.rs
grep -q 'entering_maintenance' server/src/api/nexus/monitoring/mod.rs
grep -q 'leaving_maintenance' server/src/api/nexus/monitoring/mod.rs
grep -q 'NexusMonitoring' ui/src/nexus/mod.rs
grep -q '"Monitoring"' ui/src/main_menu.rs
grep -q '"/monitoring/device"' ui/src/nexus/monitoring.rs

test -f services/nexus-icmp-collector.service
grep -q 'otelcol-contrib' services/nexus-icmp-collector.service
grep -q '^User=www-data$' services/nexus-icmp-collector.service
grep -q '^Group=www-data$' services/nexus-icmp-collector.service
grep -q -- '--config=/usr/lib/proxmox/nexus-icmp-collector-runtime.yaml' services/nexus-icmp-collector.service
if grep -q 'CAP_NET_RAW' services/nexus-icmp-collector.service; then
    echo 'collector must not retain CAP_NET_RAW; blackbox owns ICMP probing' >&2
    exit 1
fi

test -f services/nexus-icmp-collector-runtime.yaml
grep -q "host: '127.0.0.1'" services/nexus-icmp-collector-runtime.yaml
grep -q 'port: 8889' services/nexus-icmp-collector-runtime.yaml

test -f services/nexus-icmp-ping-policy.service
grep -q '^Type=oneshot$' services/nexus-icmp-ping-policy.service
grep -Fq 'id -g prometheus' services/nexus-icmp-ping-policy.service
grep -Fq '/proc/sys/net/ipv4/ping_group_range' services/nexus-icmp-ping-policy.service
if grep -Fq '0 2147483647' services/nexus-icmp-ping-policy.service; then
    echo 'ping policy must use the runtime prometheus GID, not a host-wide range' >&2
    exit 1
fi

test -f services/prometheus-blackbox-exporter-nexus.conf
grep -q '^Requires=nexus-icmp-ping-policy.service$' services/prometheus-blackbox-exporter-nexus.conf
grep -q '^After=nexus-icmp-ping-policy.service$' services/prometheus-blackbox-exporter-nexus.conf
grep -q '^NoNewPrivileges=true$' services/prometheus-blackbox-exporter-nexus.conf
if grep -q 'CAP_NET_RAW' services/prometheus-blackbox-exporter-nexus.conf; then
    echo 'blackbox should use unprivileged ping sockets instead of CAP_NET_RAW' >&2
    exit 1
fi

grep -q 'nexus-icmp-collector.service' services/Makefile
grep -q 'nexus-icmp-ping-policy.service' services/Makefile
grep -q 'nexus-icmp-collector-runtime.yaml' services/Makefile
grep -q 'prometheus-blackbox-exporter.service.d' services/Makefile
if grep -q 'prometheus-blackbox-exporter-nexus-sysctl.conf' services/Makefile; then
    echo 'static ping_group_range sysctl must not be packaged' >&2
    exit 1
fi
if grep -q 'usr/lib/sysctl.d/.*nexus-blackbox-icmp.conf' debian/proxmox-datacenter-manager.install; then
    echo 'static Nexus ICMP sysctl must not be installed' >&2
    exit 1
fi
grep -q 'nexus-icmp-collector.service' debian/proxmox-datacenter-manager.install
grep -q 'nexus-icmp-ping-policy.service' debian/proxmox-datacenter-manager.install
grep -q 'usr/lib/proxmox/nexus-icmp-collector-runtime.yaml' debian/proxmox-datacenter-manager.install
grep -q 'prometheus-blackbox-exporter.service.d/nexus-icmp.conf' debian/proxmox-datacenter-manager.install

grep -q 'prometheus-blackbox-exporter' debian/control
grep -q 'nexus-icmp-collector.service' tools/nexus/build-backend-container.sh

if grep -q 'systemd-sysctl --prefix=/net/ipv4/ping_group_range' debian/proxmox-datacenter-manager.postinst; then
    echo 'postinst must not apply the invalid static ping_group_range policy' >&2
    exit 1
fi

grep -q 'systemctl is-enabled --quiet prometheus-blackbox-exporter.service' debian/proxmox-datacenter-manager.postinst
grep -q 'systemctl restart prometheus-blackbox-exporter.service' debian/proxmox-datacenter-manager.postinst
grep -q 'systemctl is-enabled --quiet nexus-icmp-collector.service' debian/proxmox-datacenter-manager.postinst
grep -q 'systemctl restart nexus-icmp-collector.service' debian/proxmox-datacenter-manager.postinst
if grep -q 'systemctl try-restart .*\(prometheus-blackbox-exporter\|nexus-icmp-collector\)' debian/proxmox-datacenter-manager.postinst; then
    echo 'configured monitoring services must not use try-restart on upgrade' >&2
    exit 1
fi

cargo fmt --all -- --check

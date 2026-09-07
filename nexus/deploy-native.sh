#!/usr/bin/env bash
set -Eeuo pipefail

cd "$(dirname "$0")"

[[ "$(id -u)" -eq 0 ]] || { echo "deploy-native.sh must run as root" >&2; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "python3 is required" >&2; exit 1; }
command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }
command -v systemctl >/dev/null 2>&1 || { echo "systemd is required" >&2; exit 1; }

python3 - <<'PY'
import sys
if sys.version_info < (3, 12):
    raise SystemExit("Python 3.12+ is required")
PY

if [[ ! -f .env ]]; then
  cp .env.example .env
  echo "Created nexus/.env from .env.example. Review environment-specific values before deployment." >&2
  exit 2
fi

install -d -m 0755 /var/lib/dreamteclabs-nexus/prometheus

create_venv() {
  rm -rf .venv
  python3 -m venv .venv
}

if [[ ! -x .venv/bin/python ]]; then
  if ! create_venv; then
    if command -v apt-get >/dev/null 2>&1; then
      echo "Python venv support is missing; installing python3-venv..." >&2
      apt-get update
      DEBIAN_FRONTEND=noninteractive apt-get install -y python3-venv
      create_venv
    else
      echo "Python venv support is required. Install the venv package for the active Python version." >&2
      exit 1
    fi
  fi
fi

.venv/bin/python -m pip install --upgrade pip
.venv/bin/python -m pip install .

repo_dir="$(pwd)"
venv_python="${repo_dir}/.venv/bin/python"

otel_bin="$(command -v otelcol-contrib || true)"
if [[ -z "$otel_bin" ]]; then
  otel_bin="$(command -v otelcol || true)"
fi
[[ -n "$otel_bin" ]] || {
  echo "OpenTelemetry Collector is required (otelcol-contrib or otelcol)." >&2
  exit 1
}

cat >/etc/systemd/system/nexus-core.service <<EOF
[Unit]
Description=DreamtecLabs Nexus Core
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=${repo_dir}
EnvironmentFile=${repo_dir}/.env
ExecStart=${venv_python} -m uvicorn nexus_core.main:app --host 0.0.0.0 --port 8081
Restart=on-failure
RestartSec=3
TimeoutStopSec=20
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

cat >/etc/systemd/system/nexus-otel.service <<EOF
[Unit]
Description=DreamtecLabs Nexus OpenTelemetry Collector
After=network-online.target nexus-core.service
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=${repo_dir}
EnvironmentFile=${repo_dir}/.env
ExecStart=${otel_bin} --config=${repo_dir}/observability/otel-collector.yaml
Restart=on-failure
RestartSec=3
TimeoutStopSec=20
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable nexus-core.service nexus-otel.service >/dev/null
systemctl restart nexus-core.service

for _ in $(seq 1 18); do
  if curl -fsS http://127.0.0.1:8081/health >/dev/null 2>&1; then
    break
  fi
  if ! systemctl is-active --quiet nexus-core.service; then
    systemctl status nexus-core.service --no-pager >&2 || true
    journalctl -u nexus-core.service -n 120 --no-pager >&2 || true
    exit 1
  fi
  sleep 5
done

curl -fsS http://127.0.0.1:8081/health >/dev/null || {
  echo "Nexus Core did not become healthy within 90 seconds" >&2
  journalctl -u nexus-core.service -n 120 --no-pager >&2 || true
  exit 1
}

systemctl restart nexus-otel.service
sleep 2
systemctl is-active --quiet nexus-otel.service || {
  systemctl status nexus-otel.service --no-pager >&2 || true
  journalctl -u nexus-otel.service -n 120 --no-pager >&2 || true
  exit 1
}

echo "Nexus Core is healthy at http://127.0.0.1:8081"
systemctl --no-pager --full status nexus-core.service nexus-otel.service | sed -n '1,28p'

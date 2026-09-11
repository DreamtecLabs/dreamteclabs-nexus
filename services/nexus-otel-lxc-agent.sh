#!/usr/bin/env bash
set -euo pipefail

# ============================================================
# DreamtecLabs Nexus - OpenTelemetry LXC Agent
#
# Installs and configures OpenTelemetry Collector Contrib for
# host-metric monitoring (CPU, disk, load, filesystem, memory,
# network) inside a freshly provisioned LXC guest, sending
# metrics via OTLP to the Nexus/SigNoz collector.
#
# Pushed and executed over SSH by Nexus Core's provisioning
# bootstrap step. OTEL_VERSION/OTLP_HOST/OTLP_PORT are passed in
# as environment variables by the caller; the defaults below are
# only a fallback for manual/standalone runs.
# ============================================================

OTEL_VERSION="${OTEL_VERSION:-0.139.0}"
OTLP_HOST="${OTLP_HOST:-192.168.0.47}"
OTLP_PORT="${OTLP_PORT:-4317}"

echo
echo "============================================================"
echo " DreamtecLabs Nexus - OpenTelemetry LXC Deployment"
echo "============================================================"
echo

if [[ $EUID -ne 0 ]]; then
    echo "ERROR: Run this script as root."
    exit 1
fi

ARCH="$(dpkg --print-architecture)"

case "$ARCH" in
    amd64)
        OTEL_ARCH="amd64"
        ;;
    arm64)
        OTEL_ARCH="arm64"
        ;;
    *)
        echo "ERROR: Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

HOSTNAME_VALUE="$(hostname)"

echo "Host:          $HOSTNAME_VALUE"
echo "Architecture:  $ARCH"
echo "OTel version:  $OTEL_VERSION"
echo "OTLP endpoint: ${OTLP_HOST}:${OTLP_PORT}"
echo

echo "[1/6] Installing dependencies..."

apt-get update
apt-get install -y \
    ca-certificates \
    curl

echo
echo "[2/6] Installing otelcol-contrib ${OTEL_VERSION}..."

TMP_DEB="/tmp/otelcol-contrib_${OTEL_VERSION}.deb"

DOWNLOAD_URL="https://github.com/open-telemetry/opentelemetry-collector-releases/releases/download/v${OTEL_VERSION}/otelcol-contrib_${OTEL_VERSION}_linux_${OTEL_ARCH}.deb"

curl \
    --fail \
    --location \
    --show-error \
    --output "$TMP_DEB" \
    "$DOWNLOAD_URL"

dpkg -i "$TMP_DEB"

rm -f "$TMP_DEB"

echo
echo "[3/6] Configuring collector..."

mkdir -p /etc/otelcol-contrib

cat >/etc/otelcol-contrib/config.yaml <<EOF
receivers:
  hostmetrics:
    collection_interval: 60s
    scrapers:
      cpu: {}
      disk: {}
      load: {}
      filesystem: {}
      memory: {}
      network: {}

processors:
  resourcedetection:
    detectors: [env, system]
  batch:

exporters:
  otlp:
    endpoint: "${OTLP_HOST}:${OTLP_PORT}"
    tls:
      insecure: true

service:
  pipelines:
    metrics:
      receivers: [hostmetrics]
      processors: [resourcedetection, batch]
      exporters: [otlp]
EOF

chmod 0644 /etc/otelcol-contrib/config.yaml

cat >/etc/otelcol-contrib/otelcol-contrib.conf <<EOF
OTELCOL_OPTIONS="--config=/etc/otelcol-contrib/config.yaml"
EOF

chmod 0644 /etc/otelcol-contrib/otelcol-contrib.conf

echo
echo "[4/6] Enabling OpenTelemetry Collector..."

systemctl daemon-reload
systemctl enable otelcol-contrib
systemctl restart otelcol-contrib

sleep 3

echo
echo "[5/6] Checking service..."

if systemctl is-active --quiet otelcol-contrib; then
    echo "OK: otelcol-contrib is running."
else
    echo
    echo "ERROR: otelcol-contrib failed to start."
    echo
    systemctl status otelcol-contrib --no-pager -l || true
    echo
    journalctl -u otelcol-contrib -n 50 --no-pager || true
    exit 1
fi

echo
echo "[6/6] Checking connectivity to Nexus OTLP Gateway..."

if timeout 3 bash -c ">/dev/tcp/${OTLP_HOST}/${OTLP_PORT}" 2>/dev/null; then
    echo "OK: ${OTLP_HOST}:${OTLP_PORT} reachable."
else
    echo "WARNING: Could not connect to ${OTLP_HOST}:${OTLP_PORT}."
    echo "Collector is installed, but metrics may not reach Nexus."
fi

echo
echo "============================================================"
echo " OpenTelemetry deployment completed"
echo "============================================================"
echo

/usr/bin/otelcol-contrib --version || true

echo
systemctl status otelcol-contrib --no-pager --lines=5

echo
echo "Configuration:"
echo "  /etc/otelcol-contrib/config.yaml"
echo
echo "Logs:"
echo "  journalctl -u otelcol-contrib -f"
echo

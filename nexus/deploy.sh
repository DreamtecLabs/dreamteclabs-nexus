#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

if ! command -v docker >/dev/null 2>&1; then
  exec bash ./deploy-native.sh
fi
if ! docker compose version >/dev/null 2>&1; then
  exec bash ./deploy-native.sh
fi

if [[ ! -f .env ]]; then
  cp .env.example .env
  echo "Created nexus/.env from .env.example. Review environment-specific values before deployment." >&2
  exit 2
fi

mkdir -p /var/lib/dreamteclabs-nexus/prometheus

docker compose config --quiet
docker compose pull nexus-otel
docker compose build nexus-core
docker compose up -d --remove-orphans

container_id="$(docker compose ps -q nexus-core)"
[[ -n "$container_id" ]] || { echo "nexus-core container was not created" >&2; exit 1; }

for _ in $(seq 1 18); do
  status="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$container_id")"
  if [[ "$status" == "healthy" ]]; then
    curl -fsS http://127.0.0.1:8081/health >/dev/null
    echo "Nexus Core is healthy at http://127.0.0.1:8081"
    docker compose ps
    exit 0
  fi
  if [[ "$status" == "unhealthy" || "$status" == "exited" || "$status" == "dead" ]]; then
    docker compose logs --tail=120 nexus-core nexus-otel >&2
    exit 1
  fi
  sleep 5
done

echo "Nexus Core did not become healthy within 90 seconds" >&2
docker compose logs --tail=120 nexus-core nexus-otel >&2
exit 1

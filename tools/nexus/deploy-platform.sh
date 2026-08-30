#!/usr/bin/env bash
set -euo pipefail

if [[ ${EUID:-$(id -u)} -ne 0 ]]; then
  echo "Run this script as root." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "Working tree is not clean; refusing deployment." >&2
  exit 1
fi

sha="$(git rev-parse HEAD)"
repo="${NEXUS_GITHUB_REPOSITORY:-DreamtecLabs/dreamteclabs-nexus}"
api="https://api.github.com/repos/${repo}"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

headers=(-H 'Accept: application/vnd.github+json' -H 'X-GitHub-Api-Version: 2022-11-28')
if [[ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]]; then
  headers+=(-H "Authorization: Bearer ${GH_TOKEN:-${GITHUB_TOKEN}}")
fi

fetch_artifact() {
  local name="$1"
  local target="$2"
  local metadata="$workdir/${name}.json"
  curl -fsSL "${headers[@]}" "${api}/actions/artifacts?name=${name}&per_page=20" -o "$metadata"

  local artifact_id
  artifact_id="$(python3 - "$metadata" "$sha" <<'PY'
import json, sys
path, sha = sys.argv[1:]
with open(path, encoding='utf-8') as fh:
    data = json.load(fh)
for artifact in data.get('artifacts', []):
    run = artifact.get('workflow_run') or {}
    if not artifact.get('expired') and run.get('head_sha') == sha:
        print(artifact['id'])
        break
PY
)"

  if [[ -z "$artifact_id" ]]; then
    echo "No non-expired artifact '$name' found for commit $sha." >&2
    exit 1
  fi

  local archive="$workdir/${name}.zip"
  echo "Downloading $name (artifact $artifact_id)..."
  curl -fsSL "${headers[@]}" "${api}/actions/artifacts/${artifact_id}/zip" -o "$archive"
  mkdir -p "$target"
  python3 - "$archive" "$target" <<'PY'
import sys, zipfile
archive, target = sys.argv[1:]
with zipfile.ZipFile(archive) as zf:
    zf.extractall(target)
PY

  test -f "$target/BUILD_COMMIT"
  test "$(tr -d '[:space:]' < "$target/BUILD_COMMIT")" = "$sha"
  test -f "$target/SHA256SUMS"
  (cd "$target" && sha256sum --check SHA256SUMS)
}

server_dir="$workdir/server"
ui_dir="$workdir/ui"
server_artifact="nexus-pdm-server-${sha}"
ui_artifact="nexus-pdm-ui-${sha}"

printf 'Nexus platform deployment\nRepository: %s\nCommit:     %s\n' "$repo" "$sha"
fetch_artifact "$server_artifact" "$server_dir"
fetch_artifact "$ui_artifact" "$ui_dir"

mapfile -t server_packages < <(find "$server_dir" -maxdepth 1 -type f -name 'proxmox-datacenter-manager_*_*.deb' ! -name '*-client_*' ! -name '*-dbgsym_*' -print)
mapfile -t ui_packages < <(find "$ui_dir" -maxdepth 1 -type f -name 'proxmox-datacenter-manager-ui_*.deb' -print)
if [[ ${#server_packages[@]} -ne 1 ]]; then
  echo "Expected exactly one PDM server package; found ${#server_packages[@]}." >&2
  exit 1
fi
if [[ ${#ui_packages[@]} -ne 1 ]]; then
  echo "Expected exactly one PDM UI package; found ${#ui_packages[@]}." >&2
  exit 1
fi
server_package="${server_packages[0]}"
ui_package="${ui_packages[0]}"

test "$(dpkg-deb -f "$server_package" Package)" = "proxmox-datacenter-manager"
test "$(dpkg-deb -f "$ui_package" Package)" = "proxmox-datacenter-manager-ui"

backup_root="/var/backups/dreamteclabs-nexus-platform"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
backup_dir="$backup_root/$timestamp"
mkdir -p "$backup_dir"
dpkg-query -W -f='${Package} ${Version}\n' proxmox-datacenter-manager proxmox-datacenter-manager-ui > "$backup_dir/package-versions.txt" 2>/dev/null || true
for marker in /var/lib/dreamteclabs-nexus/server-git-sha /var/lib/dreamteclabs-nexus/ui-git-sha; do
  if [[ -f "$marker" ]]; then
    cp "$marker" "$backup_dir/$(basename "$marker").txt"
  fi
done
installed_ui="/usr/share/javascript/proxmox-datacenter-manager"
if [[ -d "$installed_ui" ]]; then
  tar -C "$(dirname "$installed_ui")" -czf "$backup_dir/proxmox-datacenter-manager-ui.tar.gz" "$(basename "$installed_ui")"
fi
for binary in \
  /usr/libexec/proxmox/proxmox-datacenter-api \
  /usr/libexec/proxmox/proxmox-datacenter-privileged-api; do
  if [[ -f "$binary" ]]; then
    cp -a "$binary" "$backup_dir/"
  fi
done

# Install the PDM control plane first so the new UI never depends on an API
# endpoint that is not present yet. Package postinst handles service reloads.
echo "Installing PDM server: $(basename "$server_package")"
dpkg --configure -a
dpkg -i "$server_package" || {
  apt-get -f install -y
  dpkg -i "$server_package"
}

echo "Installing PDM UI: $(basename "$ui_package")"
dpkg -i "$ui_package" || {
  apt-get -f install -y
  dpkg -i "$ui_package"
}

systemctl is-active --quiet proxmox-datacenter-api.service
systemctl is-active --quiet proxmox-datacenter-privileged-api.service

server_version="$(dpkg-query -W -f='${Version}' proxmox-datacenter-manager)"
ui_version="$(dpkg-query -W -f='${Version}' proxmox-datacenter-manager-ui)"
install -d -m 0755 /var/lib/dreamteclabs-nexus
printf '%s\n' "$sha" > /var/lib/dreamteclabs-nexus/server-git-sha
printf '%s\n' "$sha" > /var/lib/dreamteclabs-nexus/ui-git-sha
printf '%s\n' "$server_version" > /var/lib/dreamteclabs-nexus/server-package-version
printf '%s\n' "$ui_version" > /var/lib/dreamteclabs-nexus/ui-package-version

printf '\nNexus platform deployed successfully.\n'
printf 'Git SHA:        %s\n' "$sha"
printf 'Server version: %s\n' "$server_version"
printf 'UI version:     %s\n' "$ui_version"
printf 'Backup:         %s\n' "$backup_dir"

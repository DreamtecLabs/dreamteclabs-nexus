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
artifact_name="nexus-pdm-ui-${sha}"
api="https://api.github.com/repos/${repo}"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

headers=(-H 'Accept: application/vnd.github+json' -H 'X-GitHub-Api-Version: 2022-11-28')
if [[ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]]; then
  headers+=(-H "Authorization: Bearer ${GH_TOKEN:-${GITHUB_TOKEN}}")
fi

echo "Nexus UI artifact deployment"
echo "Repository: $repo"
echo "Commit:     $sha"
echo "Artifact:   $artifact_name"

metadata="$workdir/artifacts.json"
curl -fsSL "${headers[@]}" "${api}/actions/artifacts?name=${artifact_name}&per_page=20" -o "$metadata"

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
  echo "No non-expired UI artifact found for commit $sha." >&2
  echo "Wait for the Nexus UI GitHub Actions workflow to finish successfully, then retry." >&2
  exit 1
fi

archive="$workdir/artifact.zip"
echo "Downloading GitHub Actions artifact $artifact_id..."
curl -fsSL "${headers[@]}" "${api}/actions/artifacts/${artifact_id}/zip" -o "$archive"

mkdir -p "$workdir/package"
python3 - "$archive" "$workdir/package" <<'PY'
import sys, zipfile
archive, target = sys.argv[1:]
with zipfile.ZipFile(archive) as zf:
    zf.extractall(target)
PY

cd "$workdir/package"
if [[ ! -f BUILD_COMMIT ]]; then
  echo "Artifact is missing BUILD_COMMIT provenance." >&2
  exit 1
fi
artifact_sha="$(tr -d '[:space:]' < BUILD_COMMIT)"
if [[ "$artifact_sha" != "$sha" ]]; then
  echo "Artifact commit mismatch: expected $sha, got $artifact_sha." >&2
  exit 1
fi

if [[ ! -f SHA256SUMS ]]; then
  echo "Artifact is missing SHA256SUMS." >&2
  exit 1
fi
sha256sum --check SHA256SUMS

mapfile -t packages < <(find . -maxdepth 1 -type f -name 'proxmox-datacenter-manager-ui_*.deb' -print)
if [[ ${#packages[@]} -ne 1 ]]; then
  echo "Expected exactly one PDM UI .deb in artifact; found ${#packages[@]}." >&2
  exit 1
fi
package="${packages[0]}"
dpkg-deb --info "$package" >/dev/null
package_name="$(dpkg-deb -f "$package" Package)"
if [[ "$package_name" != "proxmox-datacenter-manager-ui" ]]; then
  echo "Unexpected package name: $package_name" >&2
  exit 1
fi

backup_root="/var/backups/dreamteclabs-nexus-ui"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
backup_dir="$backup_root/$timestamp"
installed_ui="/usr/share/javascript/proxmox-datacenter-manager"
mkdir -p "$backup_dir"
if [[ -d "$installed_ui" ]]; then
  tar -C "$(dirname "$installed_ui")" -czf "$backup_dir/proxmox-datacenter-manager-ui.tar.gz" "$(basename "$installed_ui")"
fi
dpkg-query -W -f='${Package} ${Version}\n' proxmox-datacenter-manager-ui > "$backup_dir/package-version.txt" 2>/dev/null || true
if [[ -f /var/lib/dreamteclabs-nexus/ui-git-sha ]]; then
  cp /var/lib/dreamteclabs-nexus/ui-git-sha "$backup_dir/installed-git-sha.txt"
fi

echo "Installing $(basename "$package")..."
dpkg --configure -a
dpkg -i "$package" || {
  apt-get -f install -y
  dpkg -i "$package"
}

installed_version="$(dpkg-query -W -f='${Version}' proxmox-datacenter-manager-ui)"
install -d -m 0755 /var/lib/dreamteclabs-nexus
printf '%s\n' "$sha" > /var/lib/dreamteclabs-nexus/ui-git-sha
printf '%s\n' "$installed_version" > /var/lib/dreamteclabs-nexus/ui-package-version

printf '\nNexus UI deployed successfully.\n'
printf 'Git SHA: %s\n' "$sha"
printf 'Package version: %s\n' "$installed_version"
printf 'Backup: %s\n' "$backup_dir"

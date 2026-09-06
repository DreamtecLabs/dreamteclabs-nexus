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

head_sha="$(git rev-parse HEAD)"
repo="${NEXUS_GITHUB_REPOSITORY:-DreamtecLabs/dreamteclabs-nexus}"
api="https://api.github.com/repos/${repo}"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

headers=(-H 'Accept: application/vnd.github+json' -H 'X-GitHub-Api-Version: 2022-11-28')
if [[ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]]; then
  headers+=(-H "Authorization: Bearer ${GH_TOKEN:-${GITHUB_TOKEN}}")
fi

echo "Nexus backend artifact deployment"
echo "Repository: $repo"
echo "HEAD:       $head_sha"

# The backend package job only runs on a push whose commit actually touches a
# packaged backend path, so HEAD itself frequently has no matching artifact
# (e.g. the most recent push was CI/tooling-only). Walk back through HEAD's
# ancestry and deploy the artifact for the nearest commit that has one,
# instead of requiring an exact match on the current checkout.
ancestry="$workdir/ancestry.txt"
git log --format=%H -n 1000 > "$ancestry"

resolved="$(GH_REPO="$repo" python3 - "$repo" "$ancestry" "${GH_TOKEN:-${GITHUB_TOKEN:-}}" <<'PY'
import json, sys, urllib.request

repo, shas_path, token = sys.argv[1:4]
with open(shas_path, encoding='utf-8') as fh:
    ordered_shas = [line.strip() for line in fh if line.strip()]
rank = {sha: i for i, sha in enumerate(ordered_shas)}

headers = {
    'Accept': 'application/vnd.github+json',
    'X-GitHub-Api-Version': '2022-11-28',
}
if token:
    headers['Authorization'] = f'Bearer {token}'

best_rank, best_sha, best_id = None, None, None
page = 1
while page <= 10:
    url = f'https://api.github.com/repos/{repo}/actions/artifacts?per_page=100&page={page}'
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers)) as resp:
        data = json.load(resp)
    artifacts = data.get('artifacts', [])
    if not artifacts:
        break
    for artifact in artifacts:
        if artifact.get('expired') or not artifact.get('name', '').startswith('nexus-pdm-backend-'):
            continue
        candidate_sha = (artifact.get('workflow_run') or {}).get('head_sha')
        candidate_rank = rank.get(candidate_sha)
        if candidate_rank is not None and (best_rank is None or candidate_rank < best_rank):
            best_rank, best_sha, best_id = candidate_rank, candidate_sha, artifact['id']
    if len(artifacts) < 100:
        break
    page += 1

if best_sha:
    print(f'{best_sha} {best_id}')
PY
)"

if [[ -z "$resolved" ]]; then
  echo "No non-expired backend artifact found for HEAD or any of its last $(wc -l < "$ancestry") ancestors." >&2
  echo "The backend package job only runs when a change actually affects packaged" >&2
  echo "backend paths. Wait for the Domains & Hosting GitHub Actions workflow to" >&2
  echo "finish on a commit that touches one, then retry." >&2
  exit 1
fi

read -r sha artifact_id <<<"$resolved"
if [[ "$sha" != "$head_sha" ]]; then
  echo "HEAD has no backend build; deploying the nearest ancestor that does:" >&2
  echo "  HEAD:       $head_sha" >&2
  echo "  deploying:  $sha" >&2
fi
artifact_name="nexus-pdm-backend-${sha}"
echo "Artifact:   $artifact_name"

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

mapfile -t packages < <(find . -maxdepth 1 -type f -name 'proxmox-datacenter-manager_*.deb' -print)
if [[ ${#packages[@]} -ne 1 ]]; then
  echo "Expected exactly one PDM backend .deb in artifact; found ${#packages[@]}." >&2
  exit 1
fi
package="${packages[0]}"
dpkg-deb --info "$package" >/dev/null
package_name="$(dpkg-deb -f "$package" Package)"
if [[ "$package_name" != "proxmox-datacenter-manager" ]]; then
  echo "Unexpected package name: $package_name" >&2
  exit 1
fi

backup_root="/var/backups/dreamteclabs-nexus-backend"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
backup_dir="$backup_root/$timestamp"
mkdir -p "$backup_dir"
dpkg-query -W -f='${Package} ${Version}\n' proxmox-datacenter-manager > "$backup_dir/package-version.txt" 2>/dev/null || true
if [[ -f /var/lib/dreamteclabs-nexus/backend-git-sha ]]; then
  cp /var/lib/dreamteclabs-nexus/backend-git-sha "$backup_dir/installed-git-sha.txt"
fi

echo "Installing $(basename "$package")..."
dpkg --configure -a
dpkg -i "$package" || {
  apt-get -f install -y
  dpkg -i "$package"
}

installed_version="$(dpkg-query -W -f='${Version}' proxmox-datacenter-manager)"
install -d -m 0755 /var/lib/dreamteclabs-nexus
printf '%s\n' "$sha" > /var/lib/dreamteclabs-nexus/backend-git-sha
printf '%s\n' "$installed_version" > /var/lib/dreamteclabs-nexus/backend-package-version

printf '\nNexus backend deployed successfully.\n'
printf 'Git SHA: %s\n' "$sha"
printf 'Package version: %s\n' "$installed_version"
printf 'Backup: %s\n' "$backup_dir"
printf '\nNote: unlike the UI package, this backup only records the previous version\n'
printf 'string (no static asset tree to snapshot). To roll back, re-run this script\n'
printf 'after resetting the working tree to a commit before %s.\n' "$sha"

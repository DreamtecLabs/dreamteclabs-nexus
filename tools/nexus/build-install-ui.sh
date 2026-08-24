#!/usr/bin/env bash
set -euo pipefail

if [[ ${EUID:-$(id -u)} -ne 0 ]]; then
  echo "Run this script as root." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

if [[ ! -f ui/debian/control ]]; then
  echo "PDM UI Debian control file not found." >&2
  exit 1
fi

branch="$(git branch --show-current)"
if [[ "$branch" != "feature/nexus-ui-foundation" ]]; then
  echo "Expected feature/nexus-ui-foundation, got: ${branch:-detached}" >&2
  exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "Working tree is not clean; refusing to build/install." >&2
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
git rev-parse HEAD > "$backup_dir/nexus-git-sha.txt"

apt-get update
apt-get install -y --no-install-recommends devscripts equivs git ca-certificates

mk-build-deps \
  --install \
  --remove \
  --tool 'apt-get -y --no-install-recommends' \
  ui/debian/control

git submodule update --init --recursive

make -C ui clean
make -C ui deb

package="$(find ui -maxdepth 1 -type f -name 'proxmox-datacenter-manager-ui_*.deb' -printf '%T@ %p\n' | sort -nr | head -n1 | cut -d' ' -f2-)"
if [[ -z "$package" || ! -f "$package" ]]; then
  echo "UI package was not produced." >&2
  exit 1
fi

dpkg -i "$package"

echo
echo "Nexus UI installed successfully."
echo "Git SHA: $(git rev-parse HEAD)"
echo "Package: $package"
echo "Backup: $backup_dir"
echo "Hard-refresh the PDM page in the browser to validate the UI."

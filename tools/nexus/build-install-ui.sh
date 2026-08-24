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

echo "[1/8] Repairing any interrupted APT/dpkg state"
dpkg --configure -a
apt-get -f install -y

echo "[2/8] Ensuring Proxmox development repository is available"
keyring="/usr/share/keyrings/proxmox-archive-keyring.gpg"
if [[ ! -f "$keyring" ]]; then
  echo "Missing Proxmox archive keyring: $keyring" >&2
  echo "This host must be a supported PDM/Debian Trixie installation before building Nexus." >&2
  exit 1
fi

devel_source="/etc/apt/sources.list.d/proxmox-devel.sources"
if ! grep -RqsE '^[[:space:]]*URIs:[[:space:]]+https?://download\.proxmox\.com/debian/devel/?' /etc/apt/sources.list /etc/apt/sources.list.d 2>/dev/null \
   && ! grep -RqsE '^[[:space:]]*deb[[:space:]].*download\.proxmox\.com/debian/devel/?[[:space:]]+trixie[[:space:]]+main' /etc/apt/sources.list /etc/apt/sources.list.d 2>/dev/null; then
  cat > "$devel_source" <<EOF
Types: deb
URIs: http://download.proxmox.com/debian/devel/
Suites: trixie
Components: main
Signed-By: $keyring
EOF
  echo "Added temporary build repository: $devel_source"
fi

apt-get update

echo "[3/8] Installing Debian build tooling"
apt-get install -y --no-install-recommends \
  build-essential \
  devscripts \
  equivs \
  git \
  ca-certificates

echo "[4/8] Installing the exact PDM UI Build-Depends"
if ! mk-build-deps \
  --install \
  --remove \
  --tool 'apt-get -y --no-install-recommends' \
  ui/debian/control; then
  echo >&2
  echo "Unable to satisfy PDM UI build dependencies." >&2
  echo "Unsatisfied dependencies reported by dpkg-checkbuilddeps:" >&2
  dpkg-checkbuilddeps ui/debian/control 2>&1 || true
  echo >&2
  echo "Configured Proxmox repositories:" >&2
  grep -RhsE '^(Types:|URIs:|Suites:|Components:|deb )' /etc/apt/sources.list /etc/apt/sources.list.d 2>/dev/null >&2 || true
  exit 1
fi

echo "[5/8] Installing Cargo dependencies missing from upstream Debian control"
# ui/Cargo.toml requires proxmox-subscription 1.x with the api-types feature,
# but current upstream ui/debian/control does not list that crate as a Build-Depends.
# Install the Proxmox-packaged crate explicitly rather than changing upstream packaging.
apt-get install -y --no-install-recommends \
  librust-proxmox-subscription-dev \
  'librust-proxmox-subscription+api-types-dev'

if ! find /usr/share/cargo/registry -maxdepth 1 -type d -name 'proxmox-subscription-*' -print -quit | grep -q .; then
  echo "proxmox-subscription crate is still missing from /usr/share/cargo/registry." >&2
  apt-cache policy librust-proxmox-subscription-dev 'librust-proxmox-subscription+api-types-dev' >&2 || true
  exit 1
fi

echo "[6/8] Initializing UI assets"
git submodule update --init --recursive

echo "[7/8] Building PDM/Nexus UI Debian package"
make -C ui clean
make -C ui deb

package="$(find ui -maxdepth 1 -type f -name 'proxmox-datacenter-manager-ui_*.deb' -printf '%T@ %p\n' | sort -nr | head -n1 | cut -d' ' -f2-)"
if [[ -z "$package" || ! -f "$package" ]]; then
  echo "UI package was not produced." >&2
  exit 1
fi

echo "[8/8] Installing Nexus UI package"
dpkg -i "$package"

echo
echo "Nexus UI installed successfully."
echo "Git SHA: $(git rev-parse HEAD)"
echo "Package: $package"
echo "Backup: $backup_dir"
echo "Hard-refresh the PDM page in the browser to validate the UI."

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

echo "[1/10] Repairing any interrupted APT/dpkg state"
dpkg --configure -a
apt-get -f install -y

echo "[2/10] Ensuring Proxmox development repository is available"
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

echo "[3/10] Installing Debian build tooling"
apt-get install -y --no-install-recommends \
  build-essential \
  devscripts \
  equivs \
  git \
  ca-certificates

echo "[4/10] Installing the exact PDM UI Build-Depends"
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

echo "[5/10] Installing Cargo dependencies missing from upstream Debian control"
apt-get install -y --no-install-recommends \
  librust-proxmox-subscription-dev \
  'librust-proxmox-subscription+api-types-dev'

if ! find /usr/share/cargo/registry -maxdepth 1 -type d -name 'proxmox-subscription-*' -print -quit | grep -q .; then
  echo "proxmox-subscription crate is still missing from /usr/share/cargo/registry." >&2
  apt-cache policy librust-proxmox-subscription-dev 'librust-proxmox-subscription+api-types-dev' >&2 || true
  exit 1
fi

echo "[6/10] Installing build-time data files used by upstream PDM UI"
apt-get install -y --no-install-recommends iso-codes

iso_json="/usr/share/iso-codes/json/iso_3166-1.json"
if [[ ! -f "$iso_json" ]]; then
  echo "Required ISO country data is missing after installing iso-codes: $iso_json" >&2
  dpkg -L iso-codes 2>/dev/null | grep -E 'iso_3166-1\.json$' >&2 || true
  exit 1
fi

echo "[7/10] Initializing UI assets"
git submodule update --init --recursive

echo "[8/10] Preparing a low-memory Rust/WASM build"
# Upstream Debian packaging enables fat LTO and debuginfo for the release build.
# That profile is appropriate for official release packages, but it can exceed the
# memory available on small PDM LXCs. Nexus development builds override only Cargo's
# build profile via environment variables; no upstream source/package file is modified.
existing_deb_build_options="${DEB_BUILD_OPTIONS:-}"
existing_deb_build_options="$(printf '%s' "$existing_deb_build_options" | sed -E 's/(^|[[:space:]])parallel=[0-9]+//g; s/^[[:space:]]+//; s/[[:space:]]+$//; s/[[:space:]]+/ /g')"
if [[ -n "$existing_deb_build_options" ]]; then
  export DEB_BUILD_OPTIONS="$existing_deb_build_options parallel=1"
else
  export DEB_BUILD_OPTIONS="parallel=1"
fi
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export MAKEFLAGS="-j1"
# Cargo environment configuration has higher precedence than the profile values
# appended by debian/rules. Thin LTO and no debug symbols substantially reduce peak
# memory while preserving an optimized release WASM suitable for Nexus UI testing.
export CARGO_PROFILE_RELEASE_LTO=thin
export CARGO_PROFILE_RELEASE_DEBUG=0
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1

printf 'Memory before build:\n'
free -h || true
if [[ -r /sys/fs/cgroup/memory.max ]]; then
  printf 'cgroup memory.max: %s\n' "$(cat /sys/fs/cgroup/memory.max)"
fi
if [[ -r /sys/fs/cgroup/memory.swap.max ]]; then
  printf 'cgroup memory.swap.max: %s\n' "$(cat /sys/fs/cgroup/memory.swap.max)"
fi
printf 'DEB_BUILD_OPTIONS=%s\n' "$DEB_BUILD_OPTIONS"
printf 'CARGO_BUILD_JOBS=%s\n' "$CARGO_BUILD_JOBS"
printf 'CARGO_PROFILE_RELEASE_LTO=%s\n' "$CARGO_PROFILE_RELEASE_LTO"
printf 'CARGO_PROFILE_RELEASE_DEBUG=%s\n' "$CARGO_PROFILE_RELEASE_DEBUG"

memory_bytes="$(awk '/MemTotal:/ {print $2 * 1024}' /proc/meminfo 2>/dev/null || echo 0)"
swap_bytes="$(awk '/SwapTotal:/ {print $2 * 1024}' /proc/meminfo 2>/dev/null || echo 0)"
if [[ "$memory_bytes" =~ ^[0-9]+([.][0-9]+)?$ ]] && [[ "$swap_bytes" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  memory_int="${memory_bytes%.*}"
  swap_int="${swap_bytes%.*}"
  if (( memory_int < 3758096384 && swap_int == 0 )); then
    echo "WARNING: less than 3.5 GiB RAM and no swap detected; even the low-memory profile may OOM." >&2
  fi
fi

echo "[9/10] Building PDM/Nexus UI Debian package"
make -C ui clean
if ! make -C ui deb; then
  echo >&2
  echo "Nexus UI build failed." >&2
  echo "Current memory/swap state:" >&2
  free -h >&2 || true
  if command -v journalctl >/dev/null 2>&1; then
    echo "Recent kernel OOM messages (if accessible):" >&2
    journalctl -k -n 100 --no-pager 2>/dev/null | grep -Ei 'out of memory|oom|killed process' | tail -n 20 >&2 || true
  fi
  echo "If rustc still exits with SIGKILL, raise the LXC to 6-8 GiB RAM or add swap on the Proxmox host." >&2
  exit 1
fi

package="$(find ui -maxdepth 1 -type f -name 'proxmox-datacenter-manager-ui_*.deb' -printf '%T@ %p\n' | sort -nr | head -n1 | cut -d' ' -f2-)"
if [[ -z "$package" || ! -f "$package" ]]; then
  echo "UI package was not produced." >&2
  exit 1
fi

echo "[10/10] Installing Nexus UI package"
dpkg -i "$package"

echo
echo "Nexus UI installed successfully."
echo "Git SHA: $(git rev-parse HEAD)"
echo "Package: $package"
echo "Backup: $backup_dir"
echo "Hard-refresh the PDM page in the browser to validate the UI."

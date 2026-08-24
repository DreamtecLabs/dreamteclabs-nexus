#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

if [[ ! -f ui/debian/control ]]; then
  echo "PDM UI Debian control file not found." >&2
  exit 1
fi

if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
fi
if [[ "${VERSION_CODENAME:-}" != "trixie" ]]; then
  echo "The Nexus self-hosted build runner must use Debian Trixie." >&2
  echo "Detected VERSION_CODENAME=${VERSION_CODENAME:-unknown}." >&2
  exit 1
fi

if ! command -v sudo >/dev/null 2>&1; then
  echo "sudo is required on the self-hosted runner." >&2
  exit 1
fi

if ! sudo -n true 2>/dev/null; then
  echo "The self-hosted runner user needs passwordless sudo for CI package setup." >&2
  exit 1
fi

sudo dpkg --configure -a
sudo apt-get -f install -y

keyring="/usr/share/keyrings/proxmox-archive-keyring.gpg"
if [[ ! -f "$keyring" ]]; then
  tmp_keyring="$(mktemp)"
  curl -fsSL https://enterprise.proxmox.com/debian/proxmox-archive-keyring-trixie.gpg -o "$tmp_keyring"
  sudo install -m 0644 "$tmp_keyring" "$keyring"
  rm -f "$tmp_keyring"
fi

devel_source="/etc/apt/sources.list.d/proxmox-devel.sources"
if ! grep -RqsE 'download\.proxmox\.com/debian/devel' /etc/apt/sources.list /etc/apt/sources.list.d 2>/dev/null; then
  sudo tee "$devel_source" >/dev/null <<EOF
Types: deb
URIs: http://download.proxmox.com/debian/devel/
Suites: trixie
Components: main
Signed-By: $keyring
EOF
fi

sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential \
  ca-certificates \
  curl \
  devscripts \
  equivs \
  git \
  iso-codes \
  lintian \
  librust-proxmox-subscription-dev \
  'librust-proxmox-subscription+api-types-dev'

sudo mk-build-deps \
  --install \
  --remove \
  --tool 'apt-get -y --no-install-recommends' \
  ui/debian/control

git submodule update --init --recursive

export DEB_BUILD_OPTIONS="parallel=1"
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export MAKEFLAGS="-j1"
export CARGO_PROFILE_RELEASE_LTO=thin
export CARGO_PROFILE_RELEASE_DEBUG=0
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1

printf 'Self-hosted runner build resources:\n'
free -h || true
printf 'Runner: %s\n' "${RUNNER_NAME:-unknown}"
printf 'OS: %s\n' "${RUNNER_OS:-unknown}"
printf 'Arch: %s\n' "${RUNNER_ARCH:-unknown}"

make -C ui clean
make -C ui deb

package="$(find ui -maxdepth 1 -type f -name 'proxmox-datacenter-manager-ui_*.deb' -printf '%T@ %p\n' | sort -nr | head -n1 | cut -d' ' -f2-)"
if [[ -z "$package" || ! -f "$package" ]]; then
  echo "UI package was not produced." >&2
  exit 1
fi

dpkg-deb --info "$package" >/dev/null
mkdir -p artifacts
cp -f "$package" artifacts/
sha256sum artifacts/*.deb | tee artifacts/SHA256SUMS

echo "Built artifact: $package"

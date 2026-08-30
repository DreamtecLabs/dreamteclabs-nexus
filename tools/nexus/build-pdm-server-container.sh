#!/usr/bin/env bash
set -euo pipefail

cd /workspace

if [ -n "${HOST_UID:-}" ] && [ -n "${HOST_GID:-}" ]; then
  trap 'chown -R "${HOST_UID}:${HOST_GID}" /workspace' EXIT
fi

apt-get update
apt-get install -y --no-install-recommends \
  build-essential \
  ca-certificates \
  curl \
  devscripts \
  equivs \
  git \
  gnupg \
  lintian

keyring=/usr/share/keyrings/proxmox-archive-keyring.gpg
curl -fsSL https://enterprise.proxmox.com/debian/proxmox-archive-keyring-trixie.gpg -o "$keyring"
cat >/etc/apt/sources.list.d/proxmox-devel.sources <<EOF
Types: deb
URIs: http://download.proxmox.com/debian/devel/
Suites: trixie
Components: main
Signed-By: $keyring
EOF

apt-get update
export DEB_BUILD_PROFILES=nodoc
mk-build-deps \
  --install \
  --remove \
  --tool 'apt-get -y --no-install-recommends' \
  debian/control

git config --global --add safe.directory /workspace
git submodule update --init --recursive

export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export MAKEFLAGS=-j1

rm -rf artifacts-server
mkdir -p artifacts-server

# Build only architecture-dependent packages. This compiles the real PDM API
# and avoids producing the documentation package in the CI container.
dpkg-buildpackage -B -Pnodoc -uc -us

package="$(find / -maxdepth 2 -type f -name 'proxmox-datacenter-manager_*_*.deb' ! -name '*-client_*' ! -name '*-dbgsym_*' -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n1 | cut -d' ' -f2-)"
test -n "$package"
test -f "$package"
test "$(dpkg-deb -f "$package" Package)" = "proxmox-datacenter-manager"
dpkg-deb --info "$package" >/dev/null
cp -f "$package" artifacts-server/
sha256sum artifacts-server/*.deb | sed 's#artifacts-server/##' | tee artifacts-server/SHA256SUMS
git rev-parse HEAD > artifacts-server/BUILD_COMMIT

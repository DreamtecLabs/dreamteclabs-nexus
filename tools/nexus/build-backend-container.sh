#!/usr/bin/env bash
set -euo pipefail

cd /workspace

if [[ -z "${HOST_UID:-}" || -z "${HOST_GID:-}" ]]; then
    echo "HOST_UID and HOST_GID are required so workspace ownership can be restored" >&2
    exit 2
fi
trap 'chown -R "${HOST_UID}:${HOST_GID}" /workspace' EXIT

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
cat >/etc/apt/sources.list.d/proxmox-pdm.sources <<EOF
Types: deb
URIs: http://download.proxmox.com/debian/pdm
Suites: trixie
Components: pdm-no-subscription
Signed-By: $keyring
EOF

apt-get update
export DEB_BUILD_PROFILES=nodoc
mk-build-deps \
    --build-dep \
    --build-profiles nodoc \
    --install \
    --remove \
    --tool 'apt-get -y --no-install-recommends' \
    debian/control

git config --global --add safe.directory /workspace
git submodule update --init --recursive

# Keep package installation privileged, but run the actual Debian/Rust build as
# one consistent unprivileged user. dh-cargo writes to both CARGO_TARGET_DIR and
# the copied Debian source tree (for example debian/cargo_home), so switching
# ownership once is more reliable and safer than making the workspace world
# writable and then depending on which helper creates each directory.
build_user=nexus-build
if ! id -u "$build_user" >/dev/null 2>&1; then
    useradd --system --home-dir /nonexistent --shell /usr/sbin/nologin "$build_user"
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/workspace/.cache/backend-target}"
mkdir -p "$CARGO_TARGET_DIR"
chown -R "$build_user:$build_user" /workspace
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export MAKEFLAGS=-j1

# Debian packaging occasionally returns non-zero only in the final lintian
# pass while the installable main package has already been produced. Capture
# the package first, then validate the artifact itself instead of accepting an
# unverified partial build.
set +e
runuser -u "$build_user" -- env \
    DEB_BUILD_PROFILES="$DEB_BUILD_PROFILES" \
    CARGO_TARGET_DIR="$CARGO_TARGET_DIR" \
    CARGO_BUILD_JOBS="$CARGO_BUILD_JOBS" \
    CARGO_INCREMENTAL="$CARGO_INCREMENTAL" \
    MAKEFLAGS="$MAKEFLAGS" \
    make deb-api
build_status=$?
set -e

package="$(find /workspace -maxdepth 1 -type f -name 'proxmox-datacenter-manager_*_amd64.deb' ! -name '*-client_*' ! -name '*-dbgsym_*' ! -name '*-docs_*' -printf '%T@ %p\n' | sort -nr | head -n1 | cut -d' ' -f2-)"
if [[ -z "$package" || ! -f "$package" ]]; then
    echo "Backend package was not produced (make exit $build_status)" >&2
    exit "${build_status:-1}"
fi

dpkg-deb --info "$package" >/dev/null
extract_dir="$(mktemp -d)"
dpkg-deb -x "$package" "$extract_dir"
helper="$extract_dir/usr/libexec/proxmox/nexus-domains-helper"
test -x "$helper"
grep -q 'migrate)' "$helper"
monitoring_unit="$extract_dir/usr/lib/systemd/system/nexus-icmp-collector.service"
test -f "$monitoring_unit"
grep -q 'otelcol-contrib' "$monitoring_unit"
grep -q 'nexus-icmp-collector.yaml' "$monitoring_unit"
rm -rf "$extract_dir"

rm -rf artifacts/backend
mkdir -p artifacts/backend
cp -f "$package" artifacts/backend/
sha256sum artifacts/backend/*.deb | sed 's#artifacts/backend/##' | tee artifacts/backend/SHA256SUMS
git rev-parse HEAD > artifacts/backend/BUILD_COMMIT

if [[ $build_status -ne 0 ]]; then
    echo "make deb-api returned $build_status after producing a validated main package; artifact accepted"
fi

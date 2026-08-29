#!/usr/bin/env bash
set -euo pipefail

cd /workspace

# Everything below runs as root inside this container, so the build output
# bind-mounted into /workspace ends up owned by root on the host too.
# On a persistent self-hosted runner (unlike ephemeral GitHub-hosted ones,
# which get a fresh filesystem every run) that breaks the *next* job's
# checkout, which can't clean or overwrite root-owned files. Hand ownership
# back to the invoking (non-root) runner user on every exit path - success
# or failure (`set -e` above means any command failing exits immediately,
# so a plain end-of-script chown would be skipped on a failed build).
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
  iso-codes \
  lintian

git config --global --add safe.directory /workspace

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
mk-build-deps \
  --install \
  --remove \
  --tool 'apt-get -y --no-install-recommends' \
  ui/debian/control

apt-get install -y --no-install-recommends \
  librust-proxmox-subscription-dev \
  'librust-proxmox-subscription+api-types-dev'

git submodule update --init --recursive

export DEB_BUILD_OPTIONS=parallel=1
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export MAKEFLAGS=-j1
export CARGO_PROFILE_RELEASE_LTO=thin
export CARGO_PROFILE_RELEASE_DEBUG=0
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1

free -h || true
make -C ui clean
make -C ui deb

package="$(find ui -maxdepth 1 -type f -name 'proxmox-datacenter-manager-ui_*.deb' -printf '%T@ %p\n' | sort -nr | head -n1 | cut -d' ' -f2-)"
test -n "$package"
test -f "$package"
dpkg-deb --info "$package" >/dev/null
rm -rf artifacts
mkdir -p artifacts
cp -f "$package" artifacts/
sha256sum artifacts/*.deb | sed 's#artifacts/##' | tee artifacts/SHA256SUMS
git rev-parse HEAD > artifacts/BUILD_COMMIT

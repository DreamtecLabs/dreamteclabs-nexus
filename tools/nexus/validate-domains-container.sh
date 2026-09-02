#!/usr/bin/env bash
set -euo pipefail

cd /workspace

if [[ -n "${HOST_UID:-}" && -n "${HOST_GID:-}" ]]; then
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
    gnupg

keyring=/usr/share/keyrings/proxmox-archive-keyring.gpg
curl -fsSL https://enterprise.proxmox.com/debian/proxmox-archive-keyring-trixie.gpg -o "$keyring"
cat >/etc/apt/sources.list.d/proxmox-devel.sources <<EOF
Types: deb
URIs: http://download.proxmox.com/debian/devel/
Suites: trixie
Components: main
Signed-By: $keyring
EOF

# The source package mixes Rust/development dependencies from the Proxmox
# devel repository with PDM binary packages such as libproxmox-acme-plugins.
# A clean Debian container therefore needs the public PDM repository as well.
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

export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export MAKEFLAGS=-j1

# GitHub runner workspaces can retain target/ between checkouts. Cargo normally
# fingerprints source changes correctly, but a rapid checkout can preserve a
# same-resolution mtime and reuse a stale server test binary. Always invalidate
# the server package itself while keeping dependency artifacts cached.
cargo clean -p server

# Keep the repository's Debian Cargo source replacement intact. The installed
# Proxmox/Rust build dependencies populate /usr/share/cargo/registry, including
# internal crates such as pbs-api-types that are intentionally not on crates.io.
cargo test -p server api::nexus::domains::tests --lib
cargo check -p server

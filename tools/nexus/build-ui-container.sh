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

# Keep Rust build output outside the generated Debian source tree. GitHub
# Actions restores this directory between runs, avoiding a full dependency
# rebuild for every UI change while still rebuilding changed Nexus sources.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/workspace/.cache/ui-target}"
mkdir -p "$CARGO_TARGET_DIR"

export DEB_BUILD_OPTIONS=parallel=1
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export MAKEFLAGS=-j1
export CARGO_PROFILE_RELEASE_LTO=thin
export CARGO_PROFILE_RELEASE_DEBUG=0
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1

# wasm-opt is the peak-memory phase of the PDM UI build. The self-hosted
# package job is intentionally capped at 7 GiB; Binaryen's default optimizer
# can exceed that cap after Rust compilation has already succeeded. Keep the
# production optimization pass, but use the lower-memory optimization level
# supported by the upstream Makefile instead of letting the kernel kill it.
export WASM_OPT_FLAGS="-O1"

free -h || true
# Do not run `make clean` here: it calls `cargo clean` and would erase the
# restored cross-run Cargo cache. Checkout already starts from a clean source
# tree; generated Debian build directories are recreated by the package target.
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
